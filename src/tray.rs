//! Zone de notification : icône, menu, info-bulle (mini-minuteur), notifications discrètes,
//! démarrage avec Windows. Une fenêtre cachée sur son propre thread reçoit les messages.
//! Jamais démarré en mode test (règle n° 0 : aucune notification réelle).

use std::sync::Mutex;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub enum Event {
    Show,
    NewTask,
    Pomodoro,
    Quit,
}

type Sink = Box<dyn Fn(Event) + Send>;
static SINK: Mutex<Option<Sink>> = Mutex::new(None);
static HWND_TRAY: AtomicIsize = AtomicIsize::new(0);
/// texte en attente pour l'info-bulle et la notification (posé par l'interface, lu par le thread)
static TIP: Mutex<String> = Mutex::new(String::new());
static NOTE: Mutex<Option<(String, String)>> = Mutex::new(None);

const WM_TRAY: u32 = WM_APP + 1;
const WM_SET_TIP: u32 = WM_APP + 2;
const WM_NOTIFY_NOW: u32 = WM_APP + 3;

fn send(e: Event) {
    if let Ok(s) = SINK.lock() {
        if let Some(f) = s.as_ref() {
            f(e);
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn copy_into(dst: &mut [u16], s: &str) {
    let w: Vec<u16> = s.encode_utf16().take(dst.len() - 1).collect();
    dst[..w.len()].copy_from_slice(&w);
    dst[w.len()] = 0;
}

fn base(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid
}

fn icon(hwnd: HWND, add: bool) {
    unsafe {
        let mut nid = base(hwnd);
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = LoadIconW(GetModuleHandleW(std::ptr::null()), 1 as _);
        if nid.hIcon.is_null() {
            nid.hIcon = LoadIconW(std::ptr::null_mut(), IDI_APPLICATION);
        }
        let tip = TIP.lock().map(|t| t.clone()).unwrap_or_default();
        copy_into(&mut nid.szTip, if tip.is_empty() { "Tâches" } else { &tip });
        Shell_NotifyIconW(if add { NIM_ADD } else { NIM_DELETE }, &nid);
    }
}

fn menu(hwnd: HWND) {
    unsafe {
        let m = CreatePopupMenu();
        AppendMenuW(m, MF_STRING, 1, wide("Ouvrir Tâches").as_ptr());
        AppendMenuW(m, MF_STRING, 2, wide("Nouvelle tâche").as_ptr());
        AppendMenuW(m, MF_STRING, 3, wide("Pomodoro : démarrer / arrêter").as_ptr());
        AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(m, MF_STRING, 4, wide("Quitter Tâches").as_ptr());
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd); // pour que le menu se ferme au clic à côté
        let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, std::ptr::null());
        DestroyMenu(m);
        match cmd {
            1 => send(Event::Show),
            2 => send(Event::NewTask),
            3 => send(Event::Pomodoro),
            4 => send(Event::Quit),
            _ => {}
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    static TASKBAR: AtomicIsize = AtomicIsize::new(0);
    match msg {
        WM_TRAY => {
            match (lp as u32) & 0xffff {
                WM_LBUTTONUP => send(Event::Show),
                NIN_BALLOONUSERCLICK => send(Event::Show),
                WM_RBUTTONUP | WM_CONTEXTMENU => menu(hwnd),
                _ => {}
            }
            0
        }
        WM_SET_TIP => {
            let mut nid = base(hwnd);
            nid.uFlags = NIF_TIP;
            let tip = TIP.lock().map(|t| t.clone()).unwrap_or_default();
            copy_into(&mut nid.szTip, if tip.is_empty() { "Tâches" } else { &tip });
            unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
            0
        }
        WM_NOTIFY_NOW => {
            if let Some((title, body)) = NOTE.lock().ok().and_then(|mut n| n.take()) {
                let mut nid = base(hwnd);
                nid.uFlags = NIF_INFO;
                copy_into(&mut nid.szInfoTitle, &title);
                copy_into(&mut nid.szInfo, &body);
                // discrète : sans son, respecte « ne pas déranger »
                nid.dwInfoFlags = NIIF_INFO | NIIF_NOSOUND | NIIF_RESPECT_QUIET_TIME;
                unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
            }
            0
        }
        _ => {
            // l'Explorateur a redémarré : remettre l'icône
            if msg != 0 && msg as isize == TASKBAR.load(Ordering::Relaxed) {
                icon(hwnd, true);
            }
            if msg == WM_CREATE {
                TASKBAR.store(unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) } as isize, Ordering::Relaxed);
            }
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
    }
}

pub fn start(sink: Sink) {
    *SINK.lock().unwrap_or_else(|e| e.into_inner()) = Some(sink);
    std::thread::Builder::new()
        .name("zone-notif".into())
        .spawn(|| unsafe {
            let inst = GetModuleHandleW(std::ptr::null());
            let class = wide("TachesZoneNotif");
            let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst, lpszClassName: class.as_ptr(), ..std::mem::zeroed() };
            RegisterClassW(&wc);
            // fenêtre jamais affichée (une fenêtre « message-only » ne reçoit pas TaskbarCreated)
            let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW, class.as_ptr(), wide("Tâches").as_ptr(), WS_POPUP, 0, 0, 0, 0, std::ptr::null_mut(), std::ptr::null_mut(), inst, std::ptr::null());
            if hwnd.is_null() {
                return;
            }
            HWND_TRAY.store(hwnd as isize, Ordering::Relaxed);
            icon(hwnd, true);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            icon(hwnd, false);
        })
        .ok();
}

fn post(msg: u32) {
    let h = HWND_TRAY.load(Ordering::Relaxed);
    if h != 0 {
        unsafe { PostMessageW(h as HWND, msg, 0, 0) };
    }
}

/// Info-bulle de l'icône (mini-minuteur du pomodoro). Sans effet si l'icône n'existe pas.
pub fn set_tip(text: &str) {
    if let Ok(mut t) = TIP.lock() {
        if *t == text {
            return;
        }
        *t = text.to_string();
    }
    post(WM_SET_TIP);
}

/// Notification Windows discrète (sans son). Sans effet si l'icône n'existe pas (mode test).
pub fn notify(title: &str, body: &str) {
    if HWND_TRAY.load(Ordering::Relaxed) == 0 {
        return;
    }
    if let Ok(mut n) = NOTE.lock() {
        *n = Some((title.to_string(), body.to_string()));
    }
    post(WM_NOTIFY_NOW);
}

pub fn active() -> bool {
    HWND_TRAY.load(Ordering::Relaxed) != 0
}

/// À la fermeture : retirer l'icône tout de suite.
pub fn stop() {
    let h = HWND_TRAY.load(Ordering::Relaxed);
    if h != 0 {
        icon(h as HWND, false);
    }
}

/// Démarrage avec Windows : HKCU\…\Run, valeur « Taches ». Écrit par l'exe lui-même
/// (lancé via explorer.exe, pour échapper à la virtualisation MSIX).
pub fn autostart(on: bool) {
    use windows_sys::Win32::System::Registry::*;
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let name = wide("Taches");
    unsafe {
        let mut h = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_SET_VALUE, &mut h) != 0 {
            return;
        }
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let v = wide(&format!("\"{}\" --demarrage", exe.display()));
                RegSetValueExW(h, name.as_ptr(), 0, REG_SZ, v.as_ptr() as *const u8, (v.len() * 2) as u32);
            }
        } else {
            RegDeleteValueW(h, name.as_ptr());
        }
        RegCloseKey(h);
    }
}
