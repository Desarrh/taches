#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod cli;
mod model;
mod nlp;
mod sound;
mod tray;

use egui_software_backend::{BufferMutRef, ColorFieldOrder, EguiSoftwareRender};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// Rendu CPU (comme l'Explorateur et Mémoire) : pas de contexte GPU, ~0 % au repos.
struct Gfx {
    window: Rc<Window>,
    egui: egui_winit::State,
    _context: softbuffer::Context<Rc<Window>>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    renderer: EguiSoftwareRender,
}

enum UserEvent {
    RepaintAt(Instant),
    Tray(tray::Event),
    /// arguments d'une 2e instance
    Args(Vec<String>),
}

struct Runner {
    ctx: egui::Context,
    gfx: Option<Gfx>,
    app: Option<app::App>,
    next_repaint: Option<Instant>,
    visible: bool,
    start_hidden: bool,
    args: Vec<String>,
    title: String,
    frames: u32,
}

/// Données : `%APPDATA%\Taches\taches.json` ; `TACHES_DIR` pour les tests et essais.
fn data_path() -> PathBuf {
    let dir = std::env::var_os("TACHES_DIR").map(PathBuf::from).unwrap_or_else(|| socle::paths::config_dir("Taches"));
    dir.join("taches.json")
}

fn hwnd_of(w: &Window) -> isize {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match w.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
        _ => 0,
    }
}

impl Runner {
    fn paint(&mut self) {
        let (Some(gfx), Some(app)) = (self.gfx.as_mut(), self.app.as_mut()) else { return };
        let size = gfx.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        let mut raw = gfx.egui.take_egui_input(&gfx.window);
        socle::testmode::prepare_input(&mut raw);
        let out = self.ctx.run_ui(raw, |ui| app.frame(ui));
        gfx.egui.handle_platform_output(&gfx.window, out.platform_output);
        let prims = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        if gfx.surface.resize(w, h).is_err() {
            return;
        }
        let Ok(mut buf) = gfx.surface.buffer_mut() else { return };
        buf.fill(app.pal.clear_u32());
        {
            let px: &mut [[u8; 4]] = bytemuck::cast_slice_mut(&mut buf[..]);
            let mut target = BufferMutRef::new(px, w.get() as usize, h.get() as usize);
            gfx.renderer.render(&mut target, &prims, &out.textures_delta, out.pixels_per_point);
        }
        let _ = buf.present();
        self.frames += 1;
        // essais : `TACHES_CAPTURE=fichier.png` avec le mode test → capture PrintWindow puis sortie
        if self.frames == 8 && socle::testmode::enabled() {
            if let (Some(out), Some(g)) = (std::env::var_os("TACHES_CAPTURE"), &self.gfx) {
                if let Some((w, h, px)) = socle::testmode::capture(hwnd_of(&g.window)) {
                    let _ = image::save_buffer(&out, &px, w, h, image::ColorType::Rgba8);
                }
                std::process::exit(0);
            }
        }
        if self.frames < 8 && socle::testmode::enabled() && std::env::var_os("TACHES_CAPTURE").is_some() {
            self.redraw();
        }
        self.after();
    }

    /// Applique ce que l'app a demandé (cacher, quitter, titre).
    fn after(&mut self) {
        let Some(app) = self.app.as_mut() else { return };
        let out = std::mem::take(&mut app.out);
        if out.quit {
            self.quit();
        }
        if out.hide {
            self.hide();
        }
        if let (Some(g), Some(a)) = (&self.gfx, &self.app) {
            if a.title != self.title {
                self.title = a.title.clone();
                g.window.set_title(&self.title);
            }
        }
    }

    fn quit(&mut self) {
        if let Some(a) = self.app.as_mut() {
            a.flush();
        }
        tray::stop();
        std::process::exit(0);
    }

    fn hide(&mut self) {
        if let Some(g) = &self.gfx {
            // sans icône dans la zone de notification, fermer = quitter (sinon l'app serait introuvable)
            if !tray::active() && !socle::testmode::enabled() {
                return self.quit();
            }
            if !socle::testmode::enabled() {
                g.window.set_visible(false);
            }
        }
        self.visible = false;
    }

    fn show(&mut self) {
        let Some(g) = &self.gfx else { return };
        if let Some(a) = self.app.as_mut() {
            a.on_show();
        }
        if !socle::testmode::enabled() {
            g.window.set_visible(true);
            g.window.set_minimized(false);
            g.window.focus_window();
        }
        self.visible = true;
        g.window.request_redraw();
    }

    fn redraw(&self) {
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }
}

impl ApplicationHandler<UserEvent> for Runner {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let test = socle::testmode::enabled();
        let attrs = Window::default_attributes()
            .with_title("Tâches")
            .with_inner_size(LogicalSize::new(1080.0, 700.0))
            .with_min_inner_size(LogicalSize::new(560.0, 360.0))
            .with_window_icon(window_icon())
            .with_visible(false)
            .with_active(false);
        let window = Rc::new(el.create_window(attrs).expect("création de la fenêtre"));
        if test {
            socle::testmode::hide(hwnd_of(&window));
        }
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer");
        let surface = softbuffer::Surface::new(&context, window.clone()).expect("surface");
        let egui = egui_winit::State::new(self.ctx.clone(), egui::ViewportId::ROOT, &*window, Some(window.scale_factor() as f32), None, Some(8192));
        let renderer = EguiSoftwareRender::new(ColorFieldOrder::Bgra);
        let mut a = app::App::new(&self.ctx, data_path());
        if let Some(msg) = socle::crash::pending("Taches") {
            a.toasts.error(msg);
        }
        if !test {
            tray::autostart(a.data.settings.demarrage_windows);
        }
        a.on_args(&std::mem::take(&mut self.args));
        a.essai();
        self.app = Some(a);
        self.gfx = Some(Gfx { window, egui, _context: context, surface, renderer });
        if test || !self.start_hidden {
            self.show();
        }
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(gfx) = self.gfx.as_mut() else { return };
        let resp = gfx.egui.on_window_event(&gfx.window, &event);
        match event {
            WindowEvent::CloseRequested => {
                if let Some(a) = self.app.as_mut() {
                    a.flush();
                }
                self.hide();
            }
            WindowEvent::RedrawRequested => self.paint(),
            _ => {
                if resp.repaint {
                    gfx.window.request_redraw();
                }
            }
        }
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::RepaintAt(t) => {
                if t <= Instant::now() {
                    self.redraw();
                } else {
                    self.next_repaint = Some(self.next_repaint.map_or(t, |n| n.min(t)));
                }
            }
            UserEvent::Tray(e) => match e {
                tray::Event::Show => self.show(),
                tray::Event::NewTask => {
                    self.show();
                    if let Some(a) = self.app.as_mut() {
                        a.focus_new();
                    }
                }
                tray::Event::Pomodoro => {
                    if let Some(a) = self.app.as_mut() {
                        a.pomodoro_toggle();
                    }
                    self.redraw();
                }
                tray::Event::Quit => self.quit(),
            },
            UserEvent::Args(args) => {
                if let Some(a) = self.app.as_mut() {
                    a.on_args(&args);
                }
                if !args.iter().any(|x| x == "--demarrage" || x == "--ajouter" || x == "--cocher") {
                    self.show();
                } else {
                    self.redraw();
                }
            }
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // logique de fond (rappels, pomodoro, jour suivant, enregistrement) même fenêtre cachée
        let mut next = self.next_repaint;
        if let Some(a) = self.app.as_mut() {
            let (changed, wake) = a.tick();
            if changed && self.visible {
                self.redraw();
            }
            if let Some(w) = wake {
                let t = Instant::now() + w;
                next = Some(next.map_or(t, |n| n.min(t)));
            }
        }
        self.after();
        if let Some(t) = self.next_repaint {
            if t <= Instant::now() {
                self.next_repaint = None;
                self.redraw();
            }
        }
        match next {
            Some(t) if t > Instant::now() => el.set_control_flow(ControlFlow::WaitUntil(t)),
            Some(_) => el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + std::time::Duration::from_millis(16))),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        if let Some(a) = self.app.as_mut() {
            a.flush();
        }
        tray::stop();
    }
}

fn window_icon() -> Option<winit::window::Icon> {
    let img = image::load_from_memory(include_bytes!("../assets/icon64.png")).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    winit::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}

fn main() {
    socle::crash::install("Taches", env!("CARGO_PKG_VERSION"));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let test = socle::testmode::enabled();
    if test {
        sound::MUTE.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    // commandes sans fenêtre (Lanceur) : jamais d'interface ici
    if let Some(code) = cli::run(&args, &data_path()) {
        std::process::exit(code);
    }
    let start_hidden = args.iter().any(|a| a == "--demarrage");
    let mut instance = None;
    if test {
        // règle n° 0 : ni son, ni notification, ni icône pendant les tests
        sound::MUTE.store(true, std::sync::atomic::Ordering::Relaxed);
        socle::testmode::low_priority();
    } else {
        match socle::instance::start("Taches", &args) {
            socle::instance::Start::Forwarded => return,
            socle::instance::Start::Primary(i) => instance = Some(i),
            socle::instance::Start::Alone => {}
        }
    }
    let event_loop = EventLoop::<UserEvent>::with_user_event().build().expect("boucle d’événements");
    let proxy = event_loop.create_proxy();
    if let Some(i) = instance {
        let p = proxy.clone();
        i.listen(move |a| {
            let _ = p.send_event(UserEvent::Args(a));
        });
    }
    if !test {
        let p = proxy.clone();
        tray::start(Box::new(move |e| {
            let _ = p.send_event(UserEvent::Tray(e));
        }));
    }
    let ctx = egui::Context::default();
    let p2 = proxy.clone();
    ctx.set_request_repaint_callback(move |info| {
        let _ = p2.send_event(UserEvent::RepaintAt(Instant::now() + info.delay));
    });
    let mut runner = Runner { ctx, gfx: None, app: None, next_repaint: None, visible: false, start_hidden, args, title: String::new(), frames: 0 };
    let _ = event_loop.run_app(&mut runner);
}
