//! Interface : barre latérale, liste de tâches, panneau de détail, suggestions, réglages.
//! Le dessin collecte des actions, appliquées ensuite (convention des apps).

use crate::model::*;
use crate::nlp;
use crate::sound::{self, Sound};
use crate::tray;
use chrono::{Duration as CDur, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Color32, CornerRadius, FontId, Frame, Id, Key, Layout, Margin, Modifiers, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use socle::icons::{self, Icon, Look};
use socle::theme::{self, Palette as Colors};
use socle::ui::Command;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Icônes proposées pour les listes (nom enregistré, icône).
pub const LIST_ICONS: &[(&str, Icon)] = &[
    ("liste", Icon::ListView),
    ("maison", Icon::Home),
    ("etoile", Icon::Star),
    ("dossier", Icon::Folder),
    ("eclair", Icon::Bolt),
    ("jeu", Icon::Gamepad),
    ("code", Icon::Code),
    ("fichier", Icon::File),
    ("epingle", Icon::Pin),
    ("graphe", Icon::Chart),
    ("bouclier", Icon::Shield),
    ("soleil", Icon::Sun),
    ("lune", Icon::Moon),
    ("horloge", Icon::Clock),
    ("musique", Icon::Audio),
    ("video", Icon::Video),
    ("image", Icon::Image),
    ("terminal", Icon::Terminal),
    ("cpu", Icon::Cpu),
    ("archive", Icon::Archive),
];

fn list_icon(name: &str) -> Icon {
    LIST_ICONS.iter().find(|(n, _)| *n == name).map_or(Icon::ListView, |x| x.1)
}

const GOLD: Color32 = Color32::from_rgb(0xF2, 0xB8, 0x4B);

pub fn now() -> NaiveDateTime {
    chrono::Local::now().naive_local()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    MyDay,
    Important,
    Planned,
    List(u64),
    Search,
    Pomodoro,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Travail,
    Pause,
    Longue,
}

/// Un seul minuteur : onglet Pomodoro, barre de titre et zone de notification.
struct Pomo {
    /// None = session libre (temps compté à part)
    task: Option<u64>,
    phase: Phase,
    /// durée de la phase
    len: Duration,
    /// fin prévue, minuteur en marche
    end: Instant,
    /// temps restant figé pendant une pause
    paused: Option<Duration>,
    /// cycles de travail terminés
    cycles: u32,
}

impl Pomo {
    fn left(&self) -> Duration {
        self.paused.unwrap_or_else(|| self.end.saturating_duration_since(Instant::now()))
    }
    fn running(&self) -> bool {
        self.paused.is_none()
    }
}

#[derive(Default)]
pub struct Out {
    pub hide: bool,
    pub quit: bool,
}

struct DragTask(u64);
struct DragList(u64);

enum Act {
    Select(u64),
    Toggle(u64),
    Back(u64),
    Star(u64),
    MyDay(u64),
    Delete(u64),
    Pomo(u64),
    MoveTo(u64, u64),
    Reorder(u64, u64),
    Step(u64, usize),
    AddSuggestion(u64),
}

pub struct App {
    pub pal: Colors,
    pub data: Data,
    path: PathBuf,
    pub toasts: socle::ui::Toasts,
    pub out: Out,
    pub title: String,
    dirty: Option<(Instant, Instant)>,
    writer: Option<std::thread::JoinHandle<Result<(), String>>>,
    generation: u64,
    today: NaiveDate,
    view: View,
    prev_view: View,
    sel: Option<u64>,
    new_text: String,
    focus_new: bool,
    query: String,
    focus_search: bool,
    index: socle::fuzzy::Index,
    index_ids: Vec<u64>,
    index_gen: u64,
    palette: socle::ui::Palette,
    help: bool,
    settings_open: bool,
    show_done: bool,
    suggest_open: bool,
    pomo: Option<Pomo>,
    /// tâche choisie dans l'onglet Pomodoro (None = session libre)
    pomo_task: Option<u64>,
    last_done: Option<(u64, Instant)>,
    deleted: Option<Task>,
    rename: Option<(u64, String)>,
    confirm_delete_list: Option<u64>,
    /// panneau de détail : tâche affichée et champs en cours de saisie
    detail_for: Option<u64>,
    step_text: String,
    due_text: String,
    remind_text: String,
    all_steps: bool,
    focus_title: bool,
    focus_due: bool,
    /// animations (tâche, début) : coche ou palier
    anim: Vec<(u64, Instant)>,
    /// ordre affiché (pour ↑↓)
    shown: Vec<u64>,
}

impl App {
    pub fn new(ctx: &egui::Context, path: PathBuf) -> Self {
        theme::install_fonts(ctx);
        let mut data: Data = socle::settings::load_from(&path);
        if data.list(DEFAULT_LIST).is_none() {
            data.lists.insert(0, List { id: DEFAULT_LIST, name: "Tâches".into(), icon: "maison".into() });
        }
        let dark = data.settings.sombre.unwrap_or_else(|| theme::system_dark(ctx));
        let pal = theme::palette(dark);
        theme::apply(ctx, &pal);
        no_blink(ctx);
        let today = now().date();
        data.roll(today);
        App {
            pal,
            data,
            path,
            toasts: Default::default(),
            out: Out::default(),
            title: "Tâches".into(),
            dirty: None,
            writer: None,
            generation: 1,
            today,
            view: View::MyDay,
            prev_view: View::MyDay,
            sel: None,
            new_text: String::new(),
            focus_new: true,
            query: String::new(),
            focus_search: false,
            index: socle::fuzzy::Index::new(),
            index_ids: Vec::new(),
            index_gen: 0,
            palette: Default::default(),
            help: false,
            settings_open: false,
            show_done: false,
            suggest_open: false,
            pomo: None,
            pomo_task: None,
            last_done: None,
            deleted: None,
            rename: None,
            confirm_delete_list: None,
            detail_for: None,
            step_text: String::new(),
            due_text: String::new(),
            remind_text: String::new(),
            all_steps: false,
            focus_title: false,
            focus_due: false,
            anim: Vec::new(),
            shown: Vec::new(),
        }
    }

    // ------------------------------------------------------------------ fond

    fn touch(&mut self) {
        self.generation += 1;
        let n = Instant::now();
        self.dirty = Some((self.dirty.map_or(n, |d| d.0), n));
    }

    /// Enregistrement différé : 400 ms de calme, au plus 3 s ; écriture atomique sur un thread.
    fn save_tick(&mut self) -> Option<Duration> {
        if self.writer.as_ref().is_some_and(|h| h.is_finished()) {
            if let Some(Err(e)) = self.writer.take().and_then(|h| h.join().ok()) {
                self.toasts.error(format!("Enregistrement impossible : {e}"));
            }
        }
        let (first, last) = self.dirty?;
        let due = (last + Duration::from_millis(400)).min(first + Duration::from_secs(3));
        let n = Instant::now();
        if due > n {
            return Some(due - n);
        }
        if self.writer.is_some() {
            return Some(Duration::from_millis(50));
        }
        self.dirty = None;
        match serde_json::to_vec_pretty(&self.data) {
            Ok(json) => {
                let p = self.path.clone();
                self.writer = Some(std::thread::spawn(move || socle::paths::write_atomic(&p, &json)));
            }
            Err(e) => self.toasts.error(e.to_string()),
        }
        Some(Duration::from_millis(50))
    }

    /// Écriture immédiate (fermeture).
    pub fn flush(&mut self) {
        self.pomo_stop();
        if let Some(h) = self.writer.take() {
            let _ = h.join();
        }
        if self.dirty.take().is_some() {
            if let Ok(json) = serde_json::to_vec_pretty(&self.data) {
                let _ = socle::paths::write_atomic(&self.path, &json);
            }
        }
    }

    fn ding(&self, s: Sound) {
        if self.data.settings.sons {
            sound::play(s, self.data.settings.volume);
        }
    }

    /// Logique de fond, fenêtre visible ou non. Renvoie (à redessiner, prochain réveil).
    pub fn tick(&mut self) -> (bool, Option<Duration>) {
        let now = now();
        let mut changed = false;
        let mut wake: Option<Duration> = None;
        let mut at = |d: Duration| wake = Some(wake.map_or(d, |w: Duration| w.min(d)));
        if now.date() != self.today {
            self.today = now.date();
            self.data.roll(self.today);
            self.touch();
            changed = true;
        }
        let fired = self.data.due_reminders(now);
        if !fired.is_empty() {
            for (_, title) in &fired {
                if self.data.settings.notifications {
                    tray::notify("Rappel", title);
                }
            }
            self.touch();
            changed = true;
        }
        if let Some(r) = self.data.next_reminder() {
            at((r - now).to_std().unwrap_or_default() + Duration::from_millis(20));
        }
        let midnight = (self.today + CDur::days(1)).and_hms_opt(0, 0, 1).unwrap_or(now);
        at((midnight - now).to_std().unwrap_or_default());
        if let Some(p) = self.pomo.as_ref().filter(|p| p.running()) {
            if Instant::now() >= p.end {
                self.pomo_next(false);
                changed = true;
            }
            at(Duration::from_millis(1000 - (now.nanosecond() / 1_000_000).min(999) as u64));
        }
        if let Some(d) = self.save_tick() {
            at(d);
        }
        let title = self.pomo_label().map_or("Tâches".to_string(), |l| format!("{l} — Tâches"));
        if title != self.title {
            tray::set_tip(&title);
            self.title = title;
            changed = true;
        }
        (changed, wake)
    }

    // ------------------------------------------------------------------ pomodoro

    fn minutes(&self, p: Phase) -> u64 {
        let s = &self.data.settings;
        (match p {
            Phase::Travail => s.pomo_travail,
            Phase::Pause => s.pomo_pause,
            Phase::Longue => s.pomo_longue,
        })
        .clamp(1, 240) as u64
    }

    fn phase_len(&self, p: Phase) -> Duration {
        Duration::from_secs(self.minutes(p) * 60)
    }

    fn pomo_start(&mut self, task: Option<u64>) {
        self.pomo_stop();
        let len = self.phase_len(Phase::Travail);
        self.pomo = Some(Pomo { task, phase: Phase::Travail, len, end: Instant::now() + len, paused: None, cycles: 0 });
    }

    /// Temps de travail compté sur la tâche, ou à part pour une session libre.
    fn credit(&mut self, task: Option<u64>, secs: u64) {
        if secs == 0 {
            return;
        }
        match task.and_then(|id| self.data.get_mut(id)) {
            Some(t) => t.spent += secs,
            None => self.data.free_spent += secs,
        }
        self.touch();
    }

    fn worked(p: &Pomo) -> u64 {
        if p.phase == Phase::Travail { p.len.saturating_sub(p.left()).as_secs() } else { 0 }
    }

    /// Arrêt (réinitialiser) : le temps de travail entamé est compté.
    fn pomo_stop(&mut self) {
        if let Some(p) = self.pomo.take() {
            self.credit(p.task, Self::worked(&p));
        }
    }

    /// Phase suivante : fin normale, ou « Passer » (`skip`, sans son).
    fn pomo_next(&mut self, skip: bool) {
        let Some(mut p) = self.pomo.take() else { return };
        self.credit(p.task, Self::worked(&p));
        if p.phase == Phase::Travail {
            p.cycles += 1;
            p.phase = if p.cycles % self.data.settings.pomo_cycles.max(1) == 0 { Phase::Longue } else { Phase::Pause };
        } else {
            p.phase = Phase::Travail;
        }
        p.len = self.phase_len(p.phase);
        if p.paused.is_some() {
            p.paused = Some(p.len);
        }
        p.end = Instant::now() + p.len;
        self.pomo = Some(p);
        if !skip {
            self.ding(Sound::Pomodoro);
        }
    }

    /// Démarrer / Pause / Reprendre.
    fn pomo_play_pause(&mut self) {
        match self.pomo.as_mut() {
            None => {
                let t = self.pomo_task.filter(|id| self.data.get(*id).is_some());
                self.pomo_start(t);
            }
            Some(p) => match p.paused.take() {
                Some(left) => p.end = Instant::now() + left,
                None => p.paused = Some(p.end.saturating_duration_since(Instant::now())),
            },
        }
    }

    fn phase_name(p: Phase) -> &'static str {
        match p {
            Phase::Travail => "Travail",
            Phase::Pause => "Pause",
            Phase::Longue => "Longue pause",
        }
    }

    fn pomo_label(&self) -> Option<String> {
        let p = self.pomo.as_ref()?;
        let left = p.left().as_secs();
        let pause = if p.running() { "" } else { " (en pause)" };
        Some(format!("{:02}:{:02} {}{pause}", left / 60, left % 60, Self::phase_name(p.phase).to_lowercase()))
    }

    /// Ctrl+P et menu de l'icône : démarre (tâche sélectionnée, sinon session libre) ou arrête.
    pub fn pomodoro_toggle(&mut self) {
        if self.pomo.is_some() {
            self.pomo_stop();
            self.toasts.info("Pomodoro arrêté");
            return;
        }
        let id = if self.view == View::Pomodoro { self.pomo_task } else { self.sel };
        self.pomo_task = id;
        self.pomo_start(id);
        let sur = id.and_then(|i| self.data.get(i)).map_or("session libre".to_string(), |t| format!("« {} »", t.title));
        self.toasts.info(format!("Pomodoro : {} min, {sur}", self.minutes(Phase::Travail)));
    }

    // ------------------------------------------------------------------ entrées externes

    /// Essais en mode test seulement (`TACHES_ESSAI`) : état de départ pour les captures.
    pub fn essai(&mut self) {
        if !socle::testmode::enabled() {
            return;
        }
        match std::env::var("TACHES_ESSAI").as_deref() {
            Ok("detail") => {
                self.sel = self.data.tasks.iter().find(|t| t.sequential).map(|t| t.id);
                self.new_text = "beurre demain 18h #courses !".into();
            }
            Ok("suggestions") => self.suggest_open = true,
            Ok("planifie") => self.view = View::Planned,
            Ok("pomodoro") => {
                self.view = View::Pomodoro;
                self.pomo_task = self.data.tasks.iter().find(|t| t.sequential).map(|t| t.id);
                self.pomo_start(self.pomo_task);
                if let Some(x) = self.pomo.as_mut() {
                    x.cycles = 1;
                    x.paused = Some(x.len * 3 / 5);
                }
            }
            _ => {}
        }
    }

    pub fn focus_new(&mut self) {
        self.focus_new = true;
    }

    /// Première ouverture du jour : suggestions de Ma journée.
    pub fn on_show(&mut self) {
        let n = now();
        if self.data.settings.suggestions_matin && self.data.suggested_on != Some(n.date()) {
            self.data.suggested_on = Some(n.date());
            self.touch();
            if !self.data.suggestions(n).is_empty() {
                self.view = View::MyDay;
                self.suggest_open = true;
            }
        }
    }

    /// `--ajouter "beurre demain #courses"` (aussi transmis par une 2e instance).
    pub fn on_args(&mut self, args: &[String]) {
        if let Some(i) = args.iter().position(|a| a == "--ajouter") {
            if let Some(text) = args.get(i + 1) {
                if let Some(id) = self.add_parsed(text, View::List(DEFAULT_LIST)) {
                    self.toasts.ok(format!("Ajoutée : {}", self.data.get(id).map_or("", |t| t.title.as_str())));
                }
            }
        }
        // `--cocher <id>` (Lanceur) : coche ou +1, avec le ding normal
        if let Some(id) = args.iter().position(|a| a == "--cocher").and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<u64>().ok()) {
            if self.data.get(id).is_some_and(|t| !t.is_done()) {
                self.apply(Act::Toggle(id));
            }
        }
    }

    // ------------------------------------------------------------------ actions

    fn add_parsed(&mut self, text: &str, view: View) -> Option<u64> {
        let n = now();
        let default = match view {
            View::List(id) => id,
            _ => DEFAULT_LIST,
        };
        let id = self.data.add_text(text, default, n)?;
        let today = self.today;
        if let Some(t) = self.data.get_mut(id) {
            t.important |= view == View::Important;
            if view == View::MyDay {
                t.my_day = Some(today);
            }
            if view == View::Planned && t.due.is_none() {
                t.due = Some(today);
            }
        }
        self.touch();
        Some(id)
    }

    fn apply(&mut self, a: Act) {
        let n = now();
        match a {
            Act::Select(id) => self.sel = Some(id),
            Act::Toggle(id) => {
                let was = self.data.get(id).is_some_and(|t| t.is_done());
                if let Some(s) = self.data.toggle(id, n) {
                    self.ding(s);
                    self.anim.push((id, Instant::now()));
                    if self.data.get(id).is_some_and(|t| t.is_done()) {
                        self.last_done = Some((id, Instant::now()));
                    }
                } else if was {
                    self.last_done = None;
                }
                self.touch();
            }
            Act::Back(id) => {
                self.data.count_back(id, n);
                self.touch();
            }
            Act::Star(id) => {
                if let Some(t) = self.data.get_mut(id) {
                    t.important = !t.important;
                    t.touched = n;
                }
                self.touch();
            }
            Act::MyDay(id) => {
                let today = self.today;
                if let Some(t) = self.data.get_mut(id) {
                    t.my_day = if t.in_my_day(today) { None } else { Some(today) };
                    t.touched = n;
                }
                self.touch();
            }
            Act::AddSuggestion(id) => {
                let today = self.today;
                if let Some(t) = self.data.get_mut(id) {
                    t.my_day = Some(today);
                    t.touched = n;
                }
                self.touch();
            }
            Act::Delete(id) => {
                if let Some(i) = self.data.tasks.iter().position(|t| t.id == id) {
                    let t = self.data.tasks.remove(i);
                    self.toasts.info(format!("« {} » supprimée — Ctrl+Z pour annuler", t.title));
                    self.deleted = Some(t);
                    if self.sel == Some(id) {
                        self.sel = None;
                    }
                    if let Some(p) = self.pomo.as_mut().filter(|p| p.task == Some(id)) {
                        p.task = None;
                    }
                    self.touch();
                }
            }
            Act::Pomo(id) => {
                if self.pomo.as_ref().is_some_and(|p| p.task == Some(id)) {
                    self.pomo_stop();
                } else {
                    self.pomo_task = Some(id);
                    self.pomo_start(Some(id));
                }
            }
            Act::MoveTo(id, list) => {
                if let Some(t) = self.data.get_mut(id) {
                    t.list = list;
                    t.order = i64::MAX / 2 - n.and_utc().timestamp();
                }
                self.toasts.info(format!("Déplacée vers {}", self.data.list_name(list)));
                self.touch();
            }
            Act::Reorder(id, before) => {
                if id != before {
                    self.data.reorder(id, before);
                    self.touch();
                }
            }
            Act::Step(id, i) => {
                if let Some(s) = self.data.toggle_step(id, i, n) {
                    self.ding(s);
                    self.anim.push((id, Instant::now()));
                }
                self.touch();
            }
        }
    }

    fn undo(&mut self) {
        if let Some(t) = self.deleted.take() {
            self.toasts.ok(format!("« {} » restaurée", t.title));
            self.sel = Some(t.id);
            self.data.tasks.push(t);
            self.touch();
        } else if let Some((id, at)) = self.last_done.take() {
            if at.elapsed().as_secs() < 30 && self.data.get(id).is_some_and(|t| t.is_done()) {
                self.apply(Act::Toggle(id));
            }
        } else if let Some(id) = self.sel {
            // dernier palier d'un compteur en cours
            if self.data.get(id).is_some_and(|t| t.target > 0 && t.count > 0) {
                self.apply(Act::Back(id));
            }
        }
    }

    fn drop_on(&mut self, target: View, id: u64) {
        let today = self.today;
        let n = now();
        match target {
            View::MyDay => {
                if let Some(t) = self.data.get_mut(id) {
                    t.my_day = Some(today);
                }
                self.toasts.info("Ajoutée à Ma journée");
            }
            View::Important => {
                if let Some(t) = self.data.get_mut(id) {
                    t.important = true;
                }
            }
            View::Planned => {
                if let Some(t) = self.data.get_mut(id) {
                    t.due.get_or_insert(today);
                }
            }
            View::List(l) => return self.apply(Act::MoveTo(id, l)),
            View::Pomodoro => {
                // déposer une tâche sur Pomodoro : le minuteur travaille pour elle
                self.pomo_task = Some(id);
                if let Some(p) = self.pomo.as_mut() {
                    p.task = Some(id);
                }
                return;
            }
            View::Search => {}
        }
        if let Some(t) = self.data.get_mut(id) {
            t.touched = n;
        }
        self.touch();
    }

    fn set_view(&mut self, v: View) {
        if self.view != View::Search {
            self.prev_view = self.view;
        }
        self.view = v;
        self.sel = None;
        if v != View::MyDay {
            self.suggest_open = false;
        }
    }

    fn new_list(&mut self) {
        let id = self.data.add_list("Nouvelle liste");
        self.touch();
        self.set_view(View::List(id));
        self.rename = Some((id, "Nouvelle liste".into()));
    }

    fn set_dark(&mut self, ctx: &egui::Context, dark: Option<bool>) {
        self.data.settings.sombre = dark;
        self.pal = theme::palette(dark.unwrap_or_else(|| theme::system_dark(ctx)));
        theme::apply(ctx, &self.pal);
        no_blink(ctx);
        self.touch();
    }

    // ------------------------------------------------------------------ vues

    fn matches(&self, t: &Task, v: View) -> bool {
        match v {
            View::MyDay => t.in_my_day(self.today),
            View::Important => t.important,
            View::Planned => t.due.is_some() || t.remind.is_some(),
            View::List(id) => t.list == id,
            View::Search | View::Pomodoro => false,
        }
    }

    fn count(&self, v: View) -> usize {
        self.data.tasks.iter().filter(|t| !t.is_done() && self.matches(t, v)).count()
    }

    /// (en cours, terminées) pour la vue courante.
    fn visible(&mut self, now: NaiveDateTime) -> (Vec<u64>, Vec<u64>) {
        if self.view == View::Search {
            if self.index_gen != self.generation {
                self.index.clear();
                self.index_ids.clear();
                for t in &self.data.tasks {
                    let steps: Vec<&str> = t.steps.iter().map(|s| s.title.as_str()).collect();
                    self.index.push(&format!("{} {} {}", t.title, t.note, steps.join(" ")), if t.is_done() { 0 } else { 20 });
                    self.index_ids.push(t.id);
                }
                self.index_gen = self.generation;
            }
            let hits: Vec<u64> = if self.query.trim().is_empty() { Vec::new() } else { self.index.search(&self.query, 200).into_iter().filter_map(|h| self.index_ids.get(h.index).copied()).collect() };
            let (a, d): (Vec<u64>, Vec<u64>) = hits.into_iter().partition(|id| self.data.get(*id).is_some_and(|t| !t.settled(now)));
            return (a, d);
        }
        let v = self.view;
        let mut act: Vec<&Task> = self.data.tasks.iter().filter(|t| self.matches(t, v) && !t.settled(now)).collect();
        let mut done: Vec<&Task> = self.data.tasks.iter().filter(|t| self.matches(t, v) && t.settled(now)).collect();
        if v == View::Planned {
            act.sort_by_key(|t| (t.due.or(t.remind.map(|r| r.date())), t.remind, -t.order));
        } else {
            act.sort_by_key(|t| -t.order);
        }
        done.sort_by_key(|t| std::cmp::Reverse(t.done_at));
        (act.iter().map(|t| t.id).collect(), done.iter().map(|t| t.id).collect())
    }

    fn planned_group(&self, t: &Task) -> &'static str {
        let Some(d) = t.due.or(t.remind.map(|r| r.date())) else { return "Sans date" };
        let today = self.today;
        if d < today {
            "En retard"
        } else if d == today {
            "Aujourd'hui"
        } else if d == today + CDur::days(1) {
            "Demain"
        } else if d < today + CDur::days(7) {
            "Cette semaine"
        } else {
            "Plus tard"
        }
    }

    fn view_title(&self) -> String {
        match self.view {
            View::MyDay => "Ma journée".into(),
            View::Important => "Important".into(),
            View::Planned => "Planifié".into(),
            View::List(id) => self.data.list_name(id).to_string(),
            View::Search => "Recherche".into(),
            View::Pomodoro => "Pomodoro".into(),
        }
    }

    // ------------------------------------------------------------------ commandes

    fn commands(&self) -> Vec<Command> {
        let sel = self.sel.is_some();
        let mut c = vec![
            Command::new("new", "Nouvelle tâche").group("Tâches").keys("Ctrl+N").icon(Icon::Plus),
            Command::new("toggle", "Cocher / décocher (compteur : +1)").group("Tâches").keys("Espace").icon(Icon::Check).enabled(sel),
            Command::new("back", "Compteur : -1").group("Tâches").keys("Maj+Espace").enabled(sel),
            Command::new("myday", "Ajouter à / retirer de Ma journée").group("Tâches").keys("Ctrl+D").icon(Icon::Sun).enabled(sel),
            Command::new("star", "Important").group("Tâches").keys("Ctrl+I").icon(Icon::Star).enabled(sel),
            Command::new("due", "Échéance…").group("Tâches").keys("Ctrl+T").icon(Icon::Clock).enabled(sel),
            Command::new("edit", "Modifier le titre").group("Tâches").keys("Entrée").icon(Icon::Edit).enabled(sel),
            Command::new("delete", "Supprimer la tâche").group("Tâches").keys("Suppr").icon(Icon::Trash).enabled(sel),
            Command::new("undo", "Annuler (coche, suppression, palier)").group("Tâches").keys("Ctrl+Z").icon(Icon::Undo),
            Command::new("up", "Monter la tâche").group("Tâches").keys("Alt+↑").icon(Icon::ArrowUp).enabled(sel),
            Command::new("down", "Descendre la tâche").group("Tâches").keys("Alt+↓").icon(Icon::ArrowDown).enabled(sel),
            Command::new("nav", "Tâche précédente / suivante").group("Tâches").keys("↑ ↓"),
            Command::new("pomodoro", if self.pomo.is_some() { "Arrêter le pomodoro" } else { "Démarrer un pomodoro" }).group("Pomodoro").keys("Ctrl+P").icon(Icon::Clock),
            Command::new("v:pomo", "Onglet Pomodoro").group("Pomodoro").keys("Ctrl+0").icon(Icon::Clock),
            Command::new("pomo-play", "Pomodoro : démarrer / pause").group("Pomodoro").keys("Espace (onglet)"),
            Command::new("pomo-skip", "Pomodoro : passer la phase").group("Pomodoro").enabled(self.pomo.is_some()),
            Command::new("pomo-reset", "Pomodoro : réinitialiser").group("Pomodoro").enabled(self.pomo.is_some()),
            Command::new("v:myday", "Ma journée").group("Affichage").keys("Ctrl+1").icon(Icon::Sun),
            Command::new("v:important", "Important").group("Affichage").keys("Ctrl+2").icon(Icon::Star),
            Command::new("v:planned", "Planifié").group("Affichage").keys("Ctrl+3").icon(Icon::Grid),
            Command::new("v:tasks", "Tâches").group("Affichage").keys("Ctrl+4").icon(Icon::Home),
            Command::new("lists", "Listes perso").group("Affichage").keys("Ctrl+5…9"),
            Command::new("search", "Rechercher").group("Affichage").keys("Ctrl+F").icon(Icon::Search),
            Command::new("suggest", "Suggestions pour Ma journée").group("Affichage").keys("Ctrl+J").icon(Icon::Bolt),
            Command::new("suggest-add", "Ajouter la suggestion n°").group("Affichage").keys("1…9"),
            Command::new("done", if self.show_done { "Masquer les tâches terminées" } else { "Afficher les tâches terminées" }).group("Affichage").keys("Ctrl+H").icon(Icon::Eye),
            Command::new("newlist", "Nouvelle liste").group("Listes").keys("Ctrl+Maj+N").icon(Icon::Plus),
            Command::new("rename", "Renommer la liste").group("Listes").keys("F2").icon(Icon::Edit).enabled(matches!(self.view, View::List(_))),
            Command::new("dellist", "Supprimer la liste").group("Listes").icon(Icon::Trash).enabled(matches!(self.view, View::List(id) if id != DEFAULT_LIST)),
            Command::new("settings", "Réglages").group("Réglages").keys("Ctrl+,").icon(Icon::Settings),
            Command::new("theme", "Thème clair / sombre").group("Réglages").icon(if self.pal.dark { Icon::Sun } else { Icon::Moon }),
            Command::new("mute", if self.data.settings.sons { "Couper les sons" } else { "Activer les sons" }).group("Réglages").icon(Icon::Mute),
            Command::new("palette", "Palette de commandes").group("Réglages").keys("Ctrl+K").icon(Icon::Command),
            Command::new("help", "Aide (raccourcis)").group("Réglages").keys("F1").icon(Icon::Help),
            Command::new("hide", "Fermer la fenêtre (reste dans la zone de notification)").group("Réglages").keys("Échap Échap"),
            Command::new("quit", "Quitter Tâches").group("Réglages").keys("Ctrl+Q").icon(Icon::Power),
        ];
        for l in &self.data.lists {
            c.push(Command::new(format!("list:{}", l.id), format!("Aller à {}", l.name)).group("Listes").icon(list_icon(&l.icon)));
            if sel {
                c.push(Command::new(format!("move:{}", l.id), format!("Déplacer vers {}", l.name)).group("Tâches").icon(Icon::ArrowRight));
            }
        }
        c
    }

    fn run(&mut self, ctx: &egui::Context, id: &str) {
        let sel = self.sel;
        match id {
            "new" => self.focus_new = true,
            "toggle" if self.view == View::Pomodoro => self.pomo_play_pause(),
            "toggle" => sel.into_iter().for_each(|i| self.apply(Act::Toggle(i))),
            "back" => sel.into_iter().for_each(|i| self.apply(Act::Back(i))),
            "myday" => sel.into_iter().for_each(|i| self.apply(Act::MyDay(i))),
            "star" => sel.into_iter().for_each(|i| self.apply(Act::Star(i))),
            "delete" => sel.into_iter().for_each(|i| self.apply(Act::Delete(i))),
            "due" => self.focus_due = true,
            "edit" => self.focus_title = true,
            "undo" => self.undo(),
            "up" | "down" => self.nudge(id == "up"),
            "pomodoro" => self.pomodoro_toggle(),
            "v:pomo" => self.set_view(View::Pomodoro),
            "pomo-play" => self.pomo_play_pause(),
            "pomo-skip" => self.pomo_next(true),
            "pomo-reset" => self.pomo_stop(),
            "v:myday" => self.set_view(View::MyDay),
            "v:important" => self.set_view(View::Important),
            "v:planned" => self.set_view(View::Planned),
            "v:tasks" => self.set_view(View::List(DEFAULT_LIST)),
            "search" => {
                self.set_view(View::Search);
                self.focus_search = true;
            }
            "suggest" => {
                if self.view != View::MyDay {
                    self.set_view(View::MyDay);
                    self.suggest_open = true;
                } else {
                    self.suggest_open = !self.suggest_open;
                }
            }
            "done" => self.show_done = !self.show_done,
            "newlist" => self.new_list(),
            "rename" => {
                if let View::List(l) = self.view {
                    self.rename = Some((l, self.data.list_name(l).to_string()));
                }
            }
            "dellist" => {
                if let View::List(l) = self.view {
                    self.confirm_delete_list = Some(l);
                }
            }
            "settings" => self.settings_open = true,
            "theme" => {
                let d = !self.pal.dark;
                self.set_dark(ctx, Some(d));
            }
            "mute" => {
                self.data.settings.sons = !self.data.settings.sons;
                self.touch();
            }
            "palette" => self.palette.open(),
            "help" => self.help = true,
            "hide" => self.out.hide = true,
            "quit" => self.out.quit = true,
            _ => {
                if let Some(l) = id.strip_prefix("list:").and_then(|x| x.parse().ok()) {
                    self.set_view(View::List(l));
                } else if let (Some(l), Some(t)) = (id.strip_prefix("move:").and_then(|x| x.parse().ok()), sel) {
                    self.apply(Act::MoveTo(t, l));
                }
            }
        }
    }

    /// Alt+↑ / Alt+↓ : déplace la tâche sélectionnée dans sa liste.
    fn nudge(&mut self, up: bool) {
        let Some(id) = self.sel else { return };
        let Some(i) = self.shown.iter().position(|&x| x == id) else { return };
        if up && i > 0 {
            let before = self.shown[i - 1];
            self.apply(Act::Reorder(id, before));
        } else if !up && i + 2 < self.shown.len() {
            let before = self.shown[i + 2];
            self.apply(Act::Reorder(id, before));
        } else if !up && i + 1 < self.shown.len() {
            // dernière place : passer sous la suivante
            let next = self.shown[i + 1];
            self.apply(Act::Reorder(next, id));
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let mut run: Option<&str> = None;
        ctx.input_mut(|i| {
            if i.consume_key(ctrl_shift, Key::N) {
                run = Some("newlist");
            } else if i.consume_key(Modifiers::COMMAND, Key::N) {
                run = Some("new");
            } else if i.consume_key(Modifiers::COMMAND, Key::F) {
                run = Some("search");
            } else if i.consume_key(Modifiers::COMMAND, Key::Comma) {
                run = Some("settings");
            } else if i.consume_key(Modifiers::COMMAND, Key::P) {
                run = Some("pomodoro");
            } else if i.consume_key(Modifiers::COMMAND, Key::J) {
                run = Some("suggest");
            } else if i.consume_key(Modifiers::COMMAND, Key::H) {
                run = Some("done");
            } else if i.consume_key(Modifiers::COMMAND, Key::Q) {
                run = Some("quit");
            } else if i.consume_key(Modifiers::COMMAND, Key::D) {
                run = Some("myday");
            } else if i.consume_key(Modifiers::COMMAND, Key::I) {
                run = Some("star");
            } else if i.consume_key(Modifiers::COMMAND, Key::T) {
                run = Some("due");
            } else if !typing && i.consume_key(Modifiers::COMMAND, Key::Z) {
                run = Some("undo");
            }
        });
        if let Some(r) = run {
            return self.run(ctx, r);
        }
        if let Some(d) = socle::keys::consume_ctrl_digit(ctx) {
            match d {
                0 => self.set_view(View::Pomodoro),
                1 => self.set_view(View::MyDay),
                2 => self.set_view(View::Important),
                3 => self.set_view(View::Planned),
                4 => self.set_view(View::List(DEFAULT_LIST)),
                n @ 5..=9 => {
                    let user: Vec<u64> = self.data.lists.iter().map(|l| l.id).filter(|&l| l != DEFAULT_LIST).collect();
                    if let Some(&l) = user.get(n as usize - 5) {
                        self.set_view(View::List(l));
                    }
                }
                _ => {}
            }
            return;
        }
        if typing {
            return;
        }
        // chiffres : ajouter une suggestion
        if self.suggest_open && self.view == View::MyDay {
            let sug = self.data.suggestions(now());
            for (k, d) in socle::keys::pressed(ctx) {
                if !k_mods_plain(d) {
                    continue;
                }
                if let Some(n) = socle::keys::digit(k).filter(|n| *n >= 1) {
                    if let Some(&(id, _)) = sug.get(n as usize - 1) {
                        ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
                        self.apply(Act::AddSuggestion(id));
                        return;
                    }
                }
            }
        }
        let mut r: Option<&str> = None;
        let mut mv: i32 = 0;
        ctx.input_mut(|i| {
            if i.consume_key(Modifiers::ALT, Key::ArrowUp) {
                r = Some("up");
            } else if i.consume_key(Modifiers::ALT, Key::ArrowDown) {
                r = Some("down");
            } else if i.consume_key(Modifiers::SHIFT, Key::Space) {
                r = Some("back");
            } else if i.consume_key(Modifiers::NONE, Key::Space) {
                r = Some("toggle");
            } else if i.consume_key(Modifiers::NONE, Key::Delete) {
                r = Some("delete");
            } else if i.consume_key(Modifiers::NONE, Key::Enter) {
                r = Some("edit");
            } else if i.consume_key(Modifiers::NONE, Key::F2) {
                r = Some("rename");
            } else if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                mv = -1;
            } else if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                mv = 1;
            } else if i.consume_key(Modifiers::NONE, Key::Escape) {
                r = Some("esc");
            }
        });
        if mv != 0 {
            let i = self.sel.and_then(|s| self.shown.iter().position(|&x| x == s));
            let n = self.shown.len() as i32;
            if n > 0 {
                let j = match i {
                    Some(i) => (i as i32 + mv).clamp(0, n - 1),
                    None if mv > 0 => 0,
                    None => n - 1,
                };
                self.sel = Some(self.shown[j as usize]);
            }
        }
        match r {
            Some("esc") => {
                if self.sel.is_some() {
                    self.sel = None;
                } else if self.suggest_open {
                    self.suggest_open = false;
                } else {
                    self.out.hide = true;
                }
            }
            Some(r) => self.run(ctx, r),
            None => {}
        }
    }

    // ------------------------------------------------------------------ dessin

    pub fn frame(&mut self, root: &mut Ui) {
        let ctx = root.ctx().clone();
        let n = now();
        self.anim.retain(|(_, t)| t.elapsed() < Duration::from_millis(700));
        if !self.anim.is_empty() {
            ctx.request_repaint();
        }
        if let Some(t) = self.sel {
            if self.data.get(t).is_none() {
                self.sel = None;
            }
        }
        let cmds = self.commands();
        self.palette.handle_shortcut(&ctx);
        if let Some(id) = self.palette.show(&ctx, &self.pal, &cmds) {
            self.run(&ctx, &id);
        }
        socle::ui::help_window(&ctx, &self.pal, &mut self.help, &cmds);
        let modal = self.palette.is_open() || self.help || self.settings_open || self.confirm_delete_list.is_some();
        if !modal {
            self.keys(&ctx);
        }
        if self.settings_open {
            self.settings_ui(&ctx);
        }
        if let Some(l) = self.confirm_delete_list {
            self.confirm_list_ui(&ctx, l);
        }
        let pal = self.pal.clone();
        let mut acts: Vec<Act> = Vec::new();

        egui::Panel::left("barre").exact_size(250.0).resizable(false).show_separator_line(false).frame(Frame::NONE.fill(pal.side).inner_margin(Margin::symmetric(10, 10))).show(root, |ui| {
            self.sidebar(ui, &mut acts);
        });
        egui::Panel::bottom("etat").exact_size(26.0).resizable(false).show_separator_line(false).frame(Frame::NONE.fill(pal.chrome).inner_margin(Margin::symmetric(12, 4))).show(root, |ui| {
            ui.horizontal(|ui| {
                self.toasts.ui(ui, &pal);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("Ctrl+K commandes · F1 aide").size(theme::taille::ETAT).color(pal.muted));
                    if let Some(l) = self.pomo_label() {
                        let task = self.pomo.as_ref().and_then(|p| p.task).and_then(|id| self.data.get(id)).map_or("session libre".to_string(), |t| t.title.clone());
                        ui.label(RichText::new(format!("{l} · {task}  ")).size(theme::taille::ETAT).color(pal.live));
                    }
                });
            });
        });
        let side_open = self.suggest_open && self.view == View::MyDay;
        if side_open {
            egui::Panel::right("suggestions").exact_size(340.0).resizable(false).show_separator_line(false).frame(Frame::NONE.fill(pal.surface).inner_margin(Margin::same(12))).show(root, |ui| {
                self.suggestions_ui(ui, &mut acts, n);
            });
        } else if let Some(id) = self.sel {
            egui::Panel::right("detail").exact_size(350.0).resizable(false).show_separator_line(false).frame(Frame::NONE.fill(pal.surface).inner_margin(Margin::same(12))).show(root, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.detail(ui, id, &mut acts, n));
            });
        }
        egui::CentralPanel::default().frame(Frame::NONE.fill(pal.bg).inner_margin(Margin::symmetric(20, 14))).show(root, |ui| {
            self.main(ui, &mut acts, n);
        });
        for a in acts {
            self.apply(a);
        }
        // étiquette qui suit la souris pendant un glisser
        if let Some(p) = egui::DragAndDrop::payload::<DragTask>(&ctx) {
            if let (Some(pos), Some(t)) = (ctx.pointer_latest_pos(), self.data.get(p.0)) {
                egui::Area::new(Id::unique("glisse")).order(egui::Order::Tooltip).fixed_pos(pos + vec2(14.0, 6.0)).interactable(false).show(&ctx, |ui| {
                    Frame::new().fill(pal.surface).stroke(Stroke::new(1.0, pal.accent)).corner_radius(CornerRadius::same(6)).inner_margin(Margin::symmetric(8, 4)).show(ui, |ui| {
                        ui.label(RichText::new(&t.title).color(pal.text));
                    });
                });
                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        }
    }

    fn side_item(&self, ui: &mut Ui, icon: Icon, label: &str, count: usize, active: bool, sense: Sense) -> egui::Response {
        let p = &self.pal;
        let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), sense);
        let hover_drop = r.dnd_hover_payload::<DragTask>().is_some() || r.dnd_hover_payload::<DragList>().is_some();
        let bg = if hover_drop {
            p.sel_bg
        } else if active {
            p.sel_bg
        } else if r.hovered() {
            p.hover_bg
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, CornerRadius::same(theme::arrondi::WIDGET), bg);
        if hover_drop {
            ui.painter().rect_stroke(rect, CornerRadius::same(theme::arrondi::WIDGET), Stroke::new(1.5, p.accent), StrokeKind::Inside);
        }
        if active {
            ui.painter().rect_filled(Rect::from_min_size(rect.left_top() + vec2(0.0, 8.0), vec2(3.0, 18.0)), CornerRadius::same(2), p.accent);
        }
        icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(17.0, 17.0)), icon, if active { p.accent } else { p.text });
        let w = rect.width() - 80.0;
        socle::ui::paint_text_left(ui, rect, rect.left() + 38.0, label, FontId::proportional(theme::taille::TEXTE), p.text, w);
        if count > 0 {
            socle::ui::paint_text_right(ui, rect, rect.right() - 10.0, &count.to_string(), FontId::proportional(theme::taille::META), p.muted);
        }
        r
    }

    fn sidebar(&mut self, ui: &mut Ui, acts: &mut Vec<Act>) {
        let p = self.pal.clone();
        let sid = Id::unique("recherche");
        let before = self.query.clone();
        let r = socle::ui::search_field(ui, &p, sid, &mut self.query, "Rechercher (Ctrl+F)", ui.available_width());
        if self.focus_search {
            r.request_focus();
            self.focus_search = false;
        }
        if self.query != before && !self.query.is_empty() && self.view != View::Search {
            self.set_view(View::Search);
        }
        if r.has_focus() && self.query.is_empty() && self.view == View::Search && ui.input(|i| i.key_pressed(Key::Escape)) {
            let v = self.prev_view;
            self.set_view(v);
        }
        if r.gained_focus() && !self.query.is_empty() {
            self.set_view(View::Search);
        }
        ui.add_space(8.0);
        let smart = [(View::MyDay, Icon::Sun, "Ma journée"), (View::Important, Icon::Star, "Important"), (View::Planned, Icon::Grid, "Planifié"), (View::List(DEFAULT_LIST), Icon::Home, "Tâches")];
        for (v, icon, label) in smart {
            let label = if let View::List(id) = v { self.data.list_name(id).to_string() } else { label.to_string() };
            let r = self.side_item(ui, icon, &label, self.count(v), self.view == v, Sense::click());
            if r.clicked() {
                self.set_view(v);
            }
            if let Some(d) = r.dnd_release_payload::<DragTask>() {
                self.drop_on(v, d.0);
            }
            if v == View::List(DEFAULT_LIST) {
                self.list_menu(&r, DEFAULT_LIST);
            }
        }
        {
            let v = View::Pomodoro;
            let r = self.side_item(ui, Icon::Clock, "Pomodoro", 0, self.view == v, Sense::click());
            if r.clicked() {
                self.set_view(v);
            }
            if let Some(d) = r.dnd_release_payload::<DragTask>() {
                self.drop_on(v, d.0);
            }
            if let Some(p) = &self.pomo {
                let left = p.left().as_secs();
                let col = if !p.running() { self.pal.muted } else if p.phase == Phase::Travail { self.pal.accent } else { self.pal.add };
                socle::ui::paint_text_right(ui, r.rect, r.rect.right() - 10.0, &format!("{:02}:{:02}", left / 60, left % 60), FontId::monospace(theme::taille::META), col);
            }
        }
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
        let lists: Vec<(u64, String, String)> = self.data.lists.iter().filter(|l| l.id != DEFAULT_LIST).map(|l| (l.id, l.name.clone(), l.icon.clone())).collect();
        egui::ScrollArea::vertical().max_height(ui.available_height() - 80.0).auto_shrink([false, true]).show(ui, |ui| {
            for (id, name, icon) in lists {
                if let Some((rid, text)) = &mut self.rename {
                    if *rid == id {
                        let te = ui.add(egui::TextEdit::singleline(text).desired_width(ui.available_width()).id(Id::unique(("renommer", id))));
                        if !te.has_focus() && !te.lost_focus() {
                            te.request_focus();
                        }
                        if te.lost_focus() {
                            let name = text.trim().to_string();
                            let esc = ui.input(|i| i.key_pressed(Key::Escape));
                            if !esc && !name.is_empty() {
                                if let Some(l) = self.data.lists.iter_mut().find(|l| l.id == id) {
                                    l.name = name;
                                }
                                self.touch();
                            }
                            self.rename = None;
                        }
                        continue;
                    }
                }
                let v = View::List(id);
                let r = self.side_item(ui, list_icon(&icon), &name, self.count(v), self.view == v, Sense::click_and_drag());
                if r.clicked() {
                    self.set_view(v);
                }
                if r.double_clicked() {
                    self.rename = Some((id, name.clone()));
                }
                if r.dragged() {
                    r.dnd_set_drag_payload(DragList(id));
                }
                if let Some(d) = r.dnd_release_payload::<DragTask>() {
                    acts.push(Act::MoveTo(d.0, id));
                }
                if let Some(d) = r.dnd_release_payload::<DragList>() {
                    if d.0 != id {
                        self.data.move_list(d.0, id);
                        self.touch();
                    }
                }
                self.list_menu(&r, id);
            }
        });
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.horizontal(|ui| {
                if icons::button(ui, Icon::Settings, 30.0, Look::tool(&p), "Réglages (Ctrl+,)").clicked() {
                    self.settings_open = true;
                }
                let ti = if p.dark { Icon::Sun } else { Icon::Moon };
                if icons::button(ui, ti, 30.0, Look::tool(&p), "Thème clair / sombre").clicked() {
                    let d = !p.dark;
                    self.set_dark(ui.ctx(), Some(d));
                }
                let si = if self.data.settings.sons { Icon::Volume } else { Icon::Mute };
                if icons::button(ui, si, 30.0, Look::tool(&p), "Sons").clicked() {
                    self.data.settings.sons = !self.data.settings.sons;
                    self.touch();
                }
                if icons::button(ui, Icon::Help, 30.0, Look::tool(&p), "Aide (F1)").clicked() {
                    self.help = true;
                }
            });
            if icons::wide(ui, Icon::Plus, "Nouvelle liste", "Ctrl+Maj+N", ui.available_width(), None, &p).clicked() {
                self.new_list();
            }
        });
    }

    fn list_menu(&mut self, r: &egui::Response, id: u64) {
        r.context_menu(|ui| {
            if ui.button("Renommer   F2").clicked() {
                self.rename = Some((id, self.data.list_name(id).to_string()));
                ui.close();
            }
            if id != DEFAULT_LIST {
                ui.menu_button("Icône", |ui| {
                    egui::Grid::new("icones").spacing(vec2(2.0, 2.0)).show(ui, |ui| {
                        for (i, (name, icon)) in LIST_ICONS.iter().enumerate() {
                            if icons::button(ui, *icon, 30.0, Look::tool(&self.pal), name).clicked() {
                                if let Some(l) = self.data.lists.iter_mut().find(|l| l.id == id) {
                                    l.icon = name.to_string();
                                }
                                self.touch();
                                ui.close();
                            }
                            if i % 5 == 4 {
                                ui.end_row();
                            }
                        }
                    });
                });
                let i = self.data.lists.iter().position(|l| l.id == id).unwrap_or(0);
                if i > 1 && ui.button("Monter").clicked() {
                    self.data.lists.swap(i, i - 1);
                    self.touch();
                    ui.close();
                }
                if i + 1 < self.data.lists.len() && ui.button("Descendre").clicked() {
                    self.data.lists.swap(i, i + 1);
                    self.touch();
                    ui.close();
                }
                ui.separator();
                if ui.button(RichText::new("Supprimer la liste…").color(self.pal.del)).clicked() {
                    self.confirm_delete_list = Some(id);
                    ui.close();
                }
            }
        });
    }

    fn main(&mut self, ui: &mut Ui, acts: &mut Vec<Act>, n: NaiveDateTime) {
        if self.view == View::Pomodoro {
            return self.pomo_view(ui);
        }
        let p = self.pal.clone();
        // en-tête
        ui.horizontal(|ui| {
            let icon = match self.view {
                View::MyDay => Icon::Sun,
                View::Important => Icon::Star,
                View::Planned => Icon::Grid,
                View::List(id) => self.data.list(id).map_or(Icon::ListView, |l| if id == DEFAULT_LIST { Icon::Home } else { list_icon(&l.icon) }),
                View::Search => Icon::Search,
                View::Pomodoro => Icon::Clock,
            };
            let (r, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
            icons::paint(ui.painter(), r, icon, p.accent);
            ui.label(RichText::new(self.view_title()).size(24.0).color(p.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.view == View::MyDay {
                    let look = if self.suggest_open { Look::active(&p) } else { Look::tool(&p) };
                    if icons::button(ui, Icon::Bolt, 32.0, look, "Suggestions (Ctrl+J)").clicked() {
                        self.suggest_open = !self.suggest_open;
                    }
                }
                let eye = if self.show_done { Look::active(&p) } else { Look::tool(&p) };
                if icons::button(ui, Icon::Eye, 32.0, eye, "Tâches terminées (Ctrl+H)").clicked() {
                    self.show_done = !self.show_done;
                }
            });
        });
        if self.view == View::MyDay {
            ui.label(RichText::new(long_date(self.today)).color(p.muted).size(theme::taille::META));
        }
        ui.add_space(8.0);
        if self.view != View::Search {
            self.add_field(ui, n);
            ui.add_space(8.0);
        }
        let (act, done) = self.visible(n);
        self.shown = act.clone();
        if self.show_done {
            self.shown.extend(done.iter().copied());
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if act.is_empty() && done.is_empty() {
                ui.add_space(30.0);
                let msg = match self.view {
                    View::MyDay => "Concentre-toi sur ta journée. Ajoute une tâche, ou ouvre les suggestions (Ctrl+J).",
                    View::Search if self.query.trim().is_empty() => "Tape pour chercher dans les titres, notes et étapes.",
                    View::Search => "Aucun résultat.",
                    _ => "Rien ici pour l'instant.",
                };
                ui.vertical_centered(|ui| ui.label(RichText::new(msg).color(p.muted)));
            }
            let mut group = "";
            for id in &act {
                if self.view == View::Planned {
                    let g = self.data.get(*id).map_or("", |t| self.planned_group(t));
                    if g != group {
                        group = g;
                        ui.add_space(6.0);
                        socle::ui::section_header(ui, &p, g);
                    }
                }
                self.row(ui, *id, acts, n);
            }
            if !done.is_empty() {
                ui.add_space(8.0);
                let label = format!("{}  Terminées ({})", if self.show_done { "▾" } else { "▸" }, done.len());
                if ui.add(egui::Label::new(RichText::new(label).color(p.muted).size(theme::taille::META)).sense(Sense::click())).clicked() {
                    self.show_done = !self.show_done;
                }
                if self.show_done {
                    for id in &done {
                        self.row(ui, *id, acts, n);
                    }
                }
            }
        });
    }

    /// Champ d'ajout avec saisie naturelle et aperçu en direct.
    fn add_field(&mut self, ui: &mut Ui, n: NaiveDateTime) {
        let p = self.pal.clone();
        let id = Id::unique("ajout");
        let w = ui.available_width();
        let font = FontId::proportional(theme::taille::TEXTE + 1.0);
        let focused = ui.memory(|m| m.has_focus(id));
        let frame = Frame::new().fill(p.surface).stroke(Stroke::new(1.0, if focused { p.accent } else { p.border })).corner_radius(CornerRadius::same(8)).inner_margin(Margin::symmetric(10, 8));
        let mut submitted = false;
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                icons::paint(ui.painter(), r, Icon::Plus, p.accent);
                let (fc, ac, abg) = (p.text, p.accent, p.sel_bg);
                let f2 = font.clone();
                let mut layouter = move |ui: &Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
                    let text = buf.as_str();
                    let parsed = nlp::parse(text, n);
                    let mut job = LayoutJob::default();
                    let mut at = 0;
                    let plain = TextFormat { font_id: f2.clone(), color: fc, ..Default::default() };
                    let hi = TextFormat { font_id: f2.clone(), color: ac, background: abg, ..Default::default() };
                    for (range, _) in parsed.spans.iter() {
                        if range.start < at || range.end > text.len() {
                            continue;
                        }
                        job.append(&text[at..range.start], 0.0, plain.clone());
                        job.append(&text[range.clone()], 0.0, hi.clone());
                        at = range.end;
                    }
                    job.append(&text[at..], 0.0, plain);
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                let r = ui.add(egui::TextEdit::singleline(&mut self.new_text).id(id).frame(Frame::NONE).hint_text("Ajouter une tâche   (ex. : beurre demain 18h #courses !)").desired_width(w - 50.0).font(font.clone()).layouter(&mut layouter));
                if self.focus_new {
                    r.request_focus();
                    self.focus_new = false;
                }
                // ↓ depuis le champ : on passe dans la liste
                if r.has_focus() && ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                    r.surrender_focus();
                    self.sel = self.shown.first().copied();
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    submitted = true;
                    r.request_focus();
                }
            });
            if !self.new_text.trim().is_empty() {
                let parsed = nlp::parse(&self.new_text, n);
                let mut bits: Vec<String> = Vec::new();
                if let Some(d) = parsed.due {
                    bits.push(format!("Échéance : {}", nlp::describe(d, None, self.today)));
                }
                if let Some(r) = parsed.remind {
                    bits.push(format!("Rappel : {}", nlp::describe(r.date(), Some(r.time()), self.today)));
                }
                if let Some(l) = &parsed.list {
                    let f = socle::fuzzy::fold(l);
                    let exists = self.data.lists.iter().any(|x| socle::fuzzy::fold(&x.name) == f);
                    bits.push(format!("Liste : {l}{}", if exists { "" } else { " (nouvelle)" }));
                }
                if parsed.important {
                    bits.push("Important".into());
                }
                if let Some(k) = parsed.target {
                    bits.push(format!("À faire {k} fois"));
                }
                if !bits.is_empty() {
                    ui.add_space(2.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(26.0);
                        ui.label(RichText::new(format!("« {} »  ·  {}", parsed.title, bits.join("  ·  "))).size(theme::taille::META).color(p.accent));
                    });
                }
            }
        });
        if submitted {
            let text = std::mem::take(&mut self.new_text);
            if self.add_parsed(&text, self.view).is_none() {
                self.new_text = text;
            }
        }
    }

    fn meta(&self, t: &Task, show_list: bool) -> Vec<(String, Color32)> {
        let p = &self.pal;
        let today = self.today;
        let mut m: Vec<(String, Color32)> = Vec::new();
        if show_list || !matches!(self.view, View::List(_)) {
            m.push((self.data.list_name(t.list).to_string(), p.muted));
        }
        if t.in_my_day(today) && self.view != View::MyDay {
            m.push(("Ma journée".into(), p.muted));
        }
        if let Some(d) = t.due {
            let late = d < today && !t.is_done();
            m.push((format!("Échéance {}", nlp::describe(d, None, today)), if late { p.del } else if d == today { p.accent } else { p.muted }));
        }
        if let Some(r) = t.remind {
            if !t.reminded || r.date() >= today {
                m.push((format!("Rappel {}", nlp::describe(r.date(), Some(r.time()), today)), p.muted));
            }
        }
        if t.target > 0 {
            let per = match t.reset {
                Reset::Jamais => "",
                Reset::Jour => " · chaque jour",
                Reset::Semaine => " · chaque semaine",
            };
            m.push((format!("{}/{}{per}", t.count, t.target), p.accent));
        }
        if !t.steps.is_empty() {
            let k = t.steps_done();
            match (t.sequential, t.next_step()) {
                (true, Some(i)) => m.push((format!("Étape {}/{} : {}", i + 1, t.steps.len(), t.steps[i].title), p.text)),
                _ => m.push((format!("{k}/{} étapes", t.steps.len()), p.muted)),
            }
        }
        if t.spent >= 60 {
            m.push((fmt_spent(t.spent), p.muted));
        }
        if !t.note.trim().is_empty() {
            m.push(("Note".into(), p.muted));
        }
        m
    }

    fn row(&mut self, ui: &mut Ui, id: u64, acts: &mut Vec<Act>, n: NaiveDateTime) {
        let Some(t) = self.data.get(id).cloned() else { return };
        let p = self.pal.clone();
        let meta = self.meta(&t, self.view == View::Search);
        let progress = if t.target > 0 {
            Some(t.count as f32 / t.target as f32)
        } else if t.sequential && !t.steps.is_empty() {
            Some(t.steps_done() as f32 / t.steps.len() as f32)
        } else {
            None
        };
        let h = if meta.is_empty() { 40.0 } else { 52.0 };
        let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click_and_drag());
        let selected = self.sel == Some(id);
        let drop_here = matches!(self.view, View::List(_)) && r.dnd_hover_payload::<DragTask>().is_some_and(|d| d.0 != id);
        let bg = if selected { p.sel_bg } else if r.hovered() { theme::blend(p.surface, p.hover_bg, 0.6) } else { p.surface };
        let card = rect.shrink2(vec2(0.0, 2.0));
        ui.painter().rect_filled(card, CornerRadius::same(theme::arrondi::WIDGET), bg);
        if drop_here {
            ui.painter().line_segment([card.left_top(), card.right_top()], Stroke::new(2.0, p.accent));
        }
        // coche / compteur
        let c = pos2(card.left() + 22.0, card.center().y);
        let rad = 10.0;
        let anim = self.anim.iter().rev().find(|a| a.0 == id).map(|a| a.1.elapsed().as_secs_f32() / 0.6);
        let done = t.is_done();
        if let Some(k) = anim.filter(|k| *k < 1.0) {
            // éclat : anneau qui s'élargit et s'estompe
            let a = (1.0 - k) * 0.8;
            ui.painter().circle_stroke(c, rad + 16.0 * k, Stroke::new(2.0, p.accent.gamma_multiply(a)));
            for i in 0..8 {
                let ang = i as f32 * std::f32::consts::TAU / 8.0;
                let d = rad + 6.0 + 14.0 * k;
                ui.painter().circle_filled(c + vec2(ang.cos(), ang.sin()) * d, 2.0 * (1.0 - k), GOLD.gamma_multiply(a));
            }
        }
        let scale = anim.filter(|k| *k < 1.0).map_or(1.0, |k| 1.0 + 0.25 * (k * std::f32::consts::PI).sin());
        let cr = rad * scale;
        if done {
            ui.painter().circle_filled(c, cr, p.accent);
            icons::paint(ui.painter(), Rect::from_center_size(c, vec2(14.0, 14.0)), Icon::Check, p.on_accent);
        } else if t.target > 0 {
            ui.painter().circle_stroke(c, cr, Stroke::new(1.5, p.border));
            let frac = t.count as f32 / t.target as f32;
            if frac > 0.0 {
                let steps = (40.0 * frac).ceil() as usize + 1;
                let pts: Vec<egui::Pos2> = (0..steps).map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * frac * i as f32 / (steps - 1) as f32;
                    c + vec2(a.cos(), a.sin()) * cr
                }).collect();
                ui.painter().add(egui::Shape::line(pts, Stroke::new(3.0, p.accent)));
            }
            ui.painter().text(c, egui::Align2::CENTER_CENTER, t.count.to_string(), FontId::proportional(10.5), p.text);
        } else {
            ui.painter().circle_stroke(c, cr, Stroke::new(1.5, if r.hovered() { p.accent } else { p.muted }));
        }
        let check = ui.interact(Rect::from_center_size(c, vec2(30.0, 30.0)), Id::unique(("coche", id)), Sense::click());
        let tip = if t.target > 0 && !done { format!("+1 ({}/{})", t.count, t.target) } else if done { "Décocher".into() } else { "Terminer".into() };
        let check = check.on_hover_text(tip);
        if check.clicked() {
            acts.push(Act::Toggle(id));
        }
        if check.secondary_clicked() && t.target > 0 {
            acts.push(Act::Back(id));
        }
        // étoile
        let star_r = Rect::from_center_size(pos2(card.right() - 20.0, card.center().y), vec2(26.0, 26.0));
        let star = ui.interact(star_r, Id::unique(("etoile", id)), Sense::click());
        icons::paint(ui.painter(), star_r.shrink(5.0), Icon::Star, if t.important { GOLD } else if star.hovered() { p.text } else { p.muted.gamma_multiply(0.6) });
        if t.important {
            ui.painter().circle_filled(star_r.center(), 3.0, GOLD);
        }
        if star.on_hover_text("Important (Ctrl+I)").clicked() {
            acts.push(Act::Star(id));
        }
        let mut right = star_r.left() - 6.0;
        // annuler pendant 5 s
        let pending = done && !t.settled(n);
        if pending {
            let left = UNDO_SECS as f32 - (n - t.done_at.unwrap_or(n)).num_milliseconds() as f32 / 1000.0;
            let ur = Rect::from_min_max(pos2(right - 70.0, card.center().y - 12.0), pos2(right, card.center().y + 12.0));
            let u = ui.interact(ur, Id::unique(("annuler", id)), Sense::click());
            ui.painter().rect_filled(ur, CornerRadius::same(6), if u.hovered() { p.hover_bg } else { p.code });
            ui.painter().text(ur.center(), egui::Align2::CENTER_CENTER, format!("Annuler {}", left.ceil().max(1.0) as i32), FontId::proportional(theme::taille::META), p.accent);
            if u.clicked() {
                acts.push(Act::Toggle(id));
            }
            right = ur.left() - 6.0;
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        if self.pomo.as_ref().is_some_and(|x| x.task == Some(id)) {
            let pr = Rect::from_center_size(pos2(right - 10.0, card.center().y), vec2(16.0, 16.0));
            icons::paint(ui.painter(), pr, Icon::Clock, p.live);
            right = pr.left() - 6.0;
        }
        // titre et méta
        let x = card.left() + 44.0;
        let maxw = right - x - 6.0;
        let ty = if meta.is_empty() { card.center().y } else { card.top() + 16.0 };
        let col = if done { p.muted } else { p.text };
        let g = if self.view == View::Search {
            socle::ui::highlighted(ui, &t.title, &self.query, FontId::proportional(theme::taille::TEXTE + 0.5), col, p.accent, maxw)
        } else {
            socle::ui::trunc(ui, &t.title, FontId::proportional(theme::taille::TEXTE + 0.5), col, maxw)
        };
        let gpos = pos2(x, ty - g.size().y / 2.0);
        let gw = g.size().x;
        ui.painter().galley(gpos, g, col);
        if done {
            ui.painter().line_segment([pos2(x, ty), pos2(x + gw, ty)], Stroke::new(1.2, p.muted));
        }
        if !meta.is_empty() {
            let mut mx = x;
            let my = card.top() + 35.0;
            for (i, (text, color)) in meta.iter().enumerate() {
                let avail = right - mx - 6.0;
                if avail < 30.0 {
                    break;
                }
                let s = if i == 0 { text.clone() } else { format!("·  {text}") };
                let g = socle::ui::trunc(ui, &s, FontId::proportional(theme::taille::META), *color, avail);
                let w = g.size().x;
                ui.painter().galley(pos2(mx, my - g.size().y / 2.0), g, *color);
                mx += w + 8.0;
            }
        }
        if let Some(f) = progress {
            let bar = Rect::from_min_size(pos2(x, card.bottom() - 4.0), vec2((right - x).min(260.0), 2.5));
            ui.painter().rect_filled(bar, CornerRadius::same(1), p.border);
            ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * f.clamp(0.0, 1.0), bar.height())), CornerRadius::same(1), p.accent);
        }
        // interactions de la ligne
        if r.clicked() {
            acts.push(Act::Select(id));
        }
        if r.double_clicked() {
            self.focus_title = true;
        }
        if r.dragged() {
            r.dnd_set_drag_payload(DragTask(id));
        }
        if matches!(self.view, View::List(_)) {
            if let Some(d) = r.dnd_release_payload::<DragTask>() {
                acts.push(Act::Reorder(d.0, id));
            }
        }
        let today = self.today;
        r.context_menu(|ui| {
            if ui.button(if t.in_my_day(today) { "Retirer de Ma journée   Ctrl+D" } else { "Ajouter à Ma journée   Ctrl+D" }).clicked() {
                acts.push(Act::MyDay(id));
                ui.close();
            }
            if ui.button(if t.important { "Retirer l'importance   Ctrl+I" } else { "Important   Ctrl+I" }).clicked() {
                acts.push(Act::Star(id));
                ui.close();
            }
            if ui.button(if self.pomo.as_ref().is_some_and(|x| x.task == Some(id)) { "Arrêter le pomodoro" } else { "Démarrer un pomodoro" }).clicked() {
                acts.push(Act::Pomo(id));
                ui.close();
            }
            ui.menu_button("Déplacer vers", |ui| {
                for l in &self.data.lists {
                    if l.id != t.list && ui.button(&l.name).clicked() {
                        acts.push(Act::MoveTo(id, l.id));
                        ui.close();
                    }
                }
            });
            ui.separator();
            if ui.button(RichText::new("Supprimer   Suppr").color(p.del)).clicked() {
                acts.push(Act::Delete(id));
                ui.close();
            }
        });
    }

    /// Onglet Pomodoro : temps restant en grand dans un anneau qui se remplit.
    fn pomo_view(&mut self, ui: &mut Ui) {
        let p = self.pal.clone();
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::Clock, p.accent);
            ui.label(RichText::new("Pomodoro").size(24.0).color(p.text));
        });
        let every = self.data.settings.pomo_cycles.max(1);
        let (phase, left, len, cycles, running, active) = match &self.pomo {
            Some(x) => (x.phase, x.left(), x.len, x.cycles, x.running(), true),
            None => (Phase::Travail, self.phase_len(Phase::Travail), self.phase_len(Phase::Travail), 0, false, false),
        };
        let rank = if phase == Phase::Travail { cycles % every + 1 } else { (cycles.max(1) - 1) % every + 1 };
        let color = if phase == Phase::Travail { p.accent } else { theme::blend(p.add, p.bg, 0.25) };
        let avail = ui.available_size();
        let d = (avail.y - 230.0).min(avail.x - 40.0).clamp(160.0, 380.0);
        ui.add_space(((avail.y - d - 210.0) / 2.0).clamp(8.0, 60.0));
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), d), Sense::hover());
        let c = rect.center();
        let rad = d / 2.0 - 10.0;
        let paint = ui.painter();
        paint.circle_stroke(c, rad, Stroke::new(12.0, theme::blend(p.border, p.bg, 0.3)));
        let frac = 1.0 - left.as_secs_f32() / len.as_secs_f32().max(1.0);
        if frac > 0.0005 {
            let k = (180.0 * frac).ceil().max(2.0) as usize;
            let pts: Vec<egui::Pos2> = (0..=k)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * frac * i as f32 / k as f32;
                    c + vec2(a.cos(), a.sin()) * rad
                })
                .collect();
            let (a0, a1) = (pts[0], pts[k]);
            paint.add(egui::Shape::line(pts, Stroke::new(12.0, color)));
            // bouts arrondis
            paint.circle_filled(a0, 6.0, color);
            paint.circle_filled(a1, 6.0, color);
        }
        let secs = left.as_secs() + u64::from(running && left.subsec_millis() > 0);
        let big = (d * 0.22).clamp(36.0, 84.0);
        paint.text(c - vec2(0.0, big * 0.15), egui::Align2::CENTER_CENTER, format!("{:02}:{:02}", secs / 60, secs % 60), FontId::proportional(big), p.text);
        let sub = if phase == Phase::Travail { format!("Travail {rank}/{every}") } else { format!("{} · {rank}/{every}", Self::phase_name(phase)) };
        paint.text(c + vec2(0.0, big * 0.55), egui::Align2::CENTER_CENTER, sub, FontId::proportional(theme::taille::TITRE + 1.0), color);
        if active && !running {
            paint.text(c + vec2(0.0, big * 0.55 + 24.0), egui::Align2::CENTER_CENTER, "en pause", FontId::proportional(theme::taille::META), p.muted);
        }
        // animation fluide seulement pendant que le minuteur tourne (CPU ~0 en pause)
        if running {
            ui.ctx().request_repaint_after(Duration::from_millis(33));
        }
        ui.add_space(18.0);
        let bw = 150.0;
        let total = bw * 3.0 + 16.0;
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
            let (icon, label) = match (active, running) {
                (false, _) => (Icon::Play, "Démarrer"),
                (true, true) => (Icon::Pause, "Pause"),
                (true, false) => (Icon::Play, "Reprendre"),
            };
            if icons::wide(ui, icon, label, "", bw, Some((color, p.on_accent)), &p).clicked() {
                self.pomo_play_pause();
            }
            ui.add_enabled_ui(active, |ui| {
                if icons::wide(ui, Icon::Next, "Passer", "", bw, None, &p).on_hover_text("Phase suivante (le travail fait est compté)").clicked() {
                    self.pomo_next(true);
                }
                if icons::wide(ui, Icon::Reset, "Réinitialiser", "", bw, None, &p).on_hover_text("Arrête et revient au début (le travail fait est compté)").clicked() {
                    self.pomo_stop();
                }
            });
        });
        ui.add_space(16.0);
        // tâche attachée (facultative)
        let today = self.today;
        let mut cands: Vec<(u64, String, bool)> = self.data.tasks.iter().filter(|t| !t.is_done()).map(|t| (t.id, t.title.clone(), t.in_my_day(today))).collect();
        cands.sort_by_key(|c| !c.2);
        let cur = self.pomo.as_ref().map_or(self.pomo_task, |x| x.task);
        let cur_name = cur.and_then(|i| self.data.get(i)).map_or("Session libre".to_string(), |t| t.title.clone());
        let mut pick = cur;
        ui.horizontal(|ui| {
            let w = 320.0;
            ui.add_space(((ui.available_width() - w - 60.0) / 2.0).max(0.0));
            ui.label(RichText::new("Tâche").color(p.muted));
            egui::ComboBox::from_id_salt("pomo-tache").selected_text(cur_name).width(w).show_ui(ui, |ui| {
                ui.selectable_value(&mut pick, None, "Session libre");
                for (id, title, _) in &cands {
                    ui.selectable_value(&mut pick, Some(*id), title);
                }
            });
        });
        if pick != cur {
            self.pomo_task = pick;
            if let Some(x) = self.pomo.as_mut() {
                x.task = pick;
            }
        }
        ui.add_space(6.0);
        let mut stats = format!("Sessions libres : {}", fmt_spent(self.data.free_spent));
        if let Some(t) = pick.and_then(|i| self.data.get(i)) {
            stats = format!("Sur « {} » : {}   ·   {stats}", t.title, fmt_spent(t.spent));
        }
        ui.vertical_centered(|ui| ui.label(RichText::new(stats).size(theme::taille::META).color(p.muted)));
    }

    fn suggestions_ui(&mut self, ui: &mut Ui, acts: &mut Vec<Act>, n: NaiveDateTime) {
        let p = self.pal.clone();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Suggestions").size(theme::taille::TITRE).color(p.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icons::button(ui, Icon::Close, 28.0, Look::tool(&p), "Fermer (Échap)").clicked() {
                    self.suggest_open = false;
                }
            });
        });
        ui.label(RichText::new("Touche 1 à 9 pour ajouter à Ma journée").size(theme::taille::META).color(p.muted));
        ui.add_space(6.0);
        let sug = self.data.suggestions(n);
        if sug.is_empty() {
            ui.add_space(20.0);
            ui.label(RichText::new("Rien à suggérer : tout est à jour.").color(p.muted));
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut last: Option<Why> = None;
            for (k, (id, why)) in sug.iter().enumerate() {
                let Some(t) = self.data.get(*id) else { continue };
                if last != Some(*why) {
                    last = Some(*why);
                    ui.add_space(6.0);
                    socle::ui::section_header(ui, &p, why.label());
                }
                let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
                ui.painter().rect_filled(rect.shrink2(vec2(0.0, 2.0)), CornerRadius::same(6), if r.hovered() { p.hover_bg } else { p.bg });
                if k < 9 {
                    let b = Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), vec2(18.0, 18.0));
                    ui.painter().rect_stroke(b, CornerRadius::same(4), Stroke::new(1.0, p.border), StrokeKind::Inside);
                    ui.painter().text(b.center(), egui::Align2::CENTER_CENTER, (k + 1).to_string(), FontId::proportional(11.0), p.muted);
                }
                let sub = match (why, t.due) {
                    (Why::EnRetard, Some(d)) => format!("{} · échéance {}", self.data.list_name(t.list), nlp::describe(d, None, self.today)),
                    _ => self.data.list_name(t.list).to_string(),
                };
                socle::ui::paint_text_left(ui, Rect::from_min_max(rect.min, pos2(rect.right(), rect.center().y + 2.0)), rect.left() + 34.0, &t.title, FontId::proportional(theme::taille::TEXTE), p.text, rect.width() - 80.0);
                socle::ui::paint_text_left(ui, Rect::from_min_max(pos2(rect.left(), rect.center().y), rect.max), rect.left() + 34.0, &sub, FontId::proportional(theme::taille::META - 1.0), if *why == Why::EnRetard { p.del } else { p.muted }, rect.width() - 80.0);
                let pr = Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), vec2(26.0, 26.0));
                let plus = ui.interact(pr, Id::unique(("sug", id)), Sense::click());
                ui.painter().rect_filled(pr, CornerRadius::same(13), if plus.hovered() { p.accent } else { p.code });
                icons::paint(ui.painter(), pr.shrink(6.0), Icon::Plus, if plus.hovered() { p.on_accent } else { p.accent });
                if plus.on_hover_text("Ajouter à Ma journée").clicked() || r.clicked() {
                    acts.push(Act::AddSuggestion(*id));
                }
            }
        });
    }

    fn detail(&mut self, ui: &mut Ui, id: u64, acts: &mut Vec<Act>, n: NaiveDateTime) {
        let p = self.pal.clone();
        let today = self.today;
        if self.detail_for != Some(id) {
            self.detail_for = Some(id);
            let t = self.data.get(id).cloned().unwrap_or_default();
            self.due_text = t.due.map(|d| nlp::describe(d, None, today)).unwrap_or_default();
            self.remind_text = t.remind.map(|r| nlp::describe(r.date(), Some(r.time()), today)).unwrap_or_default();
            self.step_text.clear();
            self.all_steps = false;
        }
        let Some(ix) = self.data.tasks.iter().position(|t| t.id == id) else { return };
        let mut changed = false;
        let mut sound: Option<Sound> = None;
        // titre
        ui.horizontal(|ui| {
            let t = &self.data.tasks[ix];
            let (c, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
            let ch = ui.interact(c, Id::unique("d-coche"), Sense::click());
            if t.is_done() {
                ui.painter().circle_filled(c.center(), 10.0, p.accent);
                icons::paint(ui.painter(), c.shrink(5.0), Icon::Check, p.on_accent);
            } else {
                ui.painter().circle_stroke(c.center(), 10.0, Stroke::new(1.5, if ch.hovered() { p.accent } else { p.muted }));
            }
            if ch.clicked() {
                acts.push(Act::Toggle(id));
            }
            let w = ui.available_width() - 70.0;
            let t = &mut self.data.tasks[ix];
            let te = ui.add(egui::TextEdit::singleline(&mut t.title).id(Id::unique("d-titre")).desired_width(w).font(FontId::proportional(theme::taille::TITRE + 1.0)).frame(Frame::NONE));
            if self.focus_title {
                te.request_focus();
                self.focus_title = false;
            }
            changed |= te.changed();
            let imp = t.important;
            if icons::button(ui, Icon::Star, 28.0, Look { fg: if imp { GOLD } else { p.muted }, ..Look::tool(&p) }, "Important (Ctrl+I)").clicked() {
                acts.push(Act::Star(id));
            }
            if icons::button(ui, Icon::Close, 28.0, Look::tool(&p), "Fermer (Échap)").clicked() {
                self.sel = None;
            }
        });
        ui.add_space(6.0);
        // étapes
        {
            let t = &self.data.tasks[ix];
            let total = t.steps.len();
            if total > 0 {
                let k = t.steps_done();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Étapes {k}/{total}")).size(theme::taille::META).color(p.muted));
                    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width() - 4.0, 4.0), Sense::hover());
                    ui.painter().rect_filled(bar, CornerRadius::same(2), p.border);
                    ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * k as f32 / total as f32, 4.0)), CornerRadius::same(2), p.accent);
                });
            }
        }
        let (seq, next) = {
            let t = &self.data.tasks[ix];
            (t.sequential, t.next_step())
        };
        let mut remove: Option<usize> = None;
        let steps_n = self.data.tasks[ix].steps.len();
        let mut hidden = 0;
        for i in 0..steps_n {
            let locked = seq && next.is_some_and(|nx| i > nx);
            if locked && !self.all_steps {
                hidden += 1;
                continue;
            }
            ui.horizontal(|ui| {
                let s = &self.data.tasks[ix].steps[i];
                let (c, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                let ch = ui.interact(c, Id::unique(("d-etape", i)), Sense::click());
                if s.done {
                    ui.painter().circle_filled(c.center(), 7.0, p.accent);
                    icons::paint(ui.painter(), c.shrink(5.0), Icon::Check, p.on_accent);
                } else {
                    ui.painter().circle_stroke(c.center(), 7.0, Stroke::new(1.3, if locked { p.border } else if ch.hovered() { p.accent } else { p.muted }));
                }
                if ch.clicked() && !locked {
                    acts.push(Act::Step(id, i));
                }
                let s = &mut self.data.tasks[ix].steps[i];
                let col = if s.done || locked { p.muted } else { p.text };
                let te = ui.add(egui::TextEdit::singleline(&mut s.title).text_color(col).desired_width(ui.available_width() - 30.0).frame(Frame::NONE).id(Id::unique(("d-etape-txt", id, i))));
                changed |= te.changed();
                if icons::button(ui, Icon::Close, 22.0, Look::tool(&p), "Supprimer l'étape").clicked() {
                    remove = Some(i);
                }
            });
        }
        if hidden > 0 {
            if ui.add(egui::Label::new(RichText::new(format!("+ {hidden} étape(s) à débloquer — voir")).size(theme::taille::META).color(p.muted)).sense(Sense::click())).clicked() {
                self.all_steps = true;
            }
        } else if seq && self.all_steps && ui.add(egui::Label::new(RichText::new("Masquer les étapes suivantes").size(theme::taille::META).color(p.muted)).sense(Sense::click())).clicked() {
            self.all_steps = false;
        }
        if let Some(i) = remove {
            self.data.tasks[ix].steps.remove(i);
            changed = true;
        }
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
            icons::paint(ui.painter(), r.shrink(3.0), Icon::Plus, p.accent);
            let te = ui.add(egui::TextEdit::singleline(&mut self.step_text).hint_text("Ajouter une étape").desired_width(ui.available_width()).frame(Frame::NONE).id(Id::unique("d-nouvelle-etape")));
            if te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !self.step_text.trim().is_empty() {
                let title = std::mem::take(&mut self.step_text).trim().to_string();
                self.data.tasks[ix].steps.push(Step { title, done: false });
                if self.data.tasks[ix].sequential {
                    self.data.tasks[ix].done_at = None;
                }
                changed = true;
                te.request_focus();
            }
        });
        {
            let t = &mut self.data.tasks[ix];
            if ui.checkbox(&mut t.sequential, "Étapes débloquées une à une").on_hover_text("Seule l'étape suivante est visible ; la dernière termine la tâche").changed() {
                changed = true;
            }
        }
        ui.add_space(8.0);
        ui.separator();
        // Ma journée
        {
            let mine = self.data.tasks[ix].in_my_day(today);
            let label = if mine { "Dans Ma journée (retirer)" } else { "Ajouter à Ma journée" };
            if icons::wide(ui, Icon::Sun, label, "Ctrl+D", ui.available_width(), None, &p).clicked() {
                acts.push(Act::MyDay(id));
            }
        }
        ui.add_space(6.0);
        // échéance
        ui.label(RichText::new("ÉCHÉANCE").size(theme::taille::ENTETE).color(p.muted));
        let due_id = Id::unique("d-echeance");
        let te = ui.add(egui::TextEdit::singleline(&mut self.due_text).id(due_id).hint_text("demain, lundi, le 12, 3 déc…").desired_width(ui.available_width()));
        if self.focus_due {
            te.request_focus();
            self.focus_due = false;
        }
        if te.has_focus() && !self.due_text.trim().is_empty() {
            let msg = match nlp::parse_when(&self.due_text, n) {
                Some((d, _)) => format!("→ {}", nlp::describe(d, None, today)),
                None => "pas compris".into(),
            };
            ui.label(RichText::new(msg).size(theme::taille::META).color(p.accent));
        }
        if te.lost_focus() {
            let t = &mut self.data.tasks[ix];
            if self.due_text.trim().is_empty() {
                t.due = None;
            } else if let Some((d, _)) = nlp::parse_when(&self.due_text, n) {
                t.due = Some(d);
            }
            self.due_text = t.due.map(|d| nlp::describe(d, None, today)).unwrap_or_default();
            changed = true;
        }
        ui.horizontal_wrapped(|ui| {
            let quick = [("Aujourd'hui", Some(today)), ("Demain", Some(today + CDur::days(1))), ("Semaine pro.", Some(week_start(today) + CDur::days(7))), ("Aucune", None)];
            for (label, d) in quick {
                if ui.small_button(label).clicked() {
                    self.data.tasks[ix].due = d;
                    self.due_text = d.map(|d| nlp::describe(d, None, today)).unwrap_or_default();
                    changed = true;
                }
            }
        });
        ui.add_space(6.0);
        // rappel
        ui.label(RichText::new("RAPPEL").size(theme::taille::ENTETE).color(p.muted));
        let te = ui.add(egui::TextEdit::singleline(&mut self.remind_text).id(Id::unique("d-rappel")).hint_text("18h, demain 9h, dans 2 h…").desired_width(ui.available_width()));
        if te.has_focus() && !self.remind_text.trim().is_empty() {
            let msg = match nlp::parse_when(&self.remind_text, n) {
                Some((d, t)) => format!("→ {}", nlp::describe(d, Some(t.unwrap_or(nine())), today)),
                None => "pas compris".into(),
            };
            ui.label(RichText::new(msg).size(theme::taille::META).color(p.accent));
        }
        if te.lost_focus() {
            let t = &mut self.data.tasks[ix];
            if self.remind_text.trim().is_empty() {
                t.remind = None;
            } else if let Some((d, tm)) = nlp::parse_when(&self.remind_text, n) {
                t.remind = Some(d.and_time(tm.unwrap_or(nine())));
                t.reminded = false;
            }
            self.remind_text = t.remind.map(|r| nlp::describe(r.date(), Some(r.time()), today)).unwrap_or_default();
            changed = true;
        }
        ui.horizontal_wrapped(|ui| {
            let in1h = n + CDur::hours(1);
            let soir = today.and_hms_opt(19, 0, 0).unwrap_or(n);
            let quick = [("Dans 1 h", Some(in1h.with_second(0).unwrap_or(in1h))), ("Ce soir", if soir > n { Some(soir) } else { None }), ("Demain 9h", Some((today + CDur::days(1)).and_time(nine()))), ("Aucun", None)];
            for (label, r) in quick {
                if label == "Ce soir" && r.is_none() {
                    continue;
                }
                if ui.small_button(label).clicked() {
                    let t = &mut self.data.tasks[ix];
                    t.remind = r;
                    t.reminded = false;
                    self.remind_text = r.map(|r| nlp::describe(r.date(), Some(r.time()), today)).unwrap_or_default();
                    changed = true;
                }
            }
        });
        ui.add_space(6.0);
        // compteur
        ui.label(RichText::new("COMPTEUR (HABITUDE)").size(theme::taille::ENTETE).color(p.muted));
        {
            let t = &mut self.data.tasks[ix];
            ui.horizontal(|ui| {
                ui.label("À faire");
                let r = ui.add(egui::DragValue::new(&mut t.target).range(0..=999).speed(0.1));
                ui.label(if t.target == 0 { "fois (0 = sans compteur)" } else { "fois" });
                if r.changed() {
                    t.count = t.count.min(t.target);
                    if t.target > 0 && t.count < t.target && t.done_at.is_some() {
                        t.done_at = None;
                    }
                    changed = true;
                }
            });
            if t.target > 0 {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}/{}", t.count, t.target)).size(theme::taille::TITRE).color(p.accent));
                    if ui.small_button("−1").clicked() {
                        acts.push(Act::Back(id));
                    }
                    if ui.small_button("+1").clicked() {
                        acts.push(Act::Toggle(id));
                    }
                    if ui.small_button("Zéro").clicked() {
                        t.count = 0;
                        t.done_at = None;
                        changed = true;
                    }
                });
                let mut cur = t.reset as usize;
                if socle::ui::segmented(ui, &p, &mut cur, &[(None, "Jamais"), (None, "Chaque jour"), (None, "Chaque semaine")]) {
                    t.reset = [Reset::Jamais, Reset::Jour, Reset::Semaine][cur];
                    t.period = match t.reset {
                        Reset::Jamais => None,
                        Reset::Jour => Some(today),
                        Reset::Semaine => Some(week_start(today)),
                    };
                    changed = true;
                }
            }
        }
        ui.add_space(6.0);
        // pomodoro
        ui.label(RichText::new("POMODORO").size(theme::taille::ENTETE).color(p.muted));
        {
            let running = self.pomo.as_ref().is_some_and(|x| x.task == Some(id));
            let label = if running { format!("Arrêter  ({})", self.pomo_label().unwrap_or_default()) } else { format!("Démarrer  ({} min)", self.minutes(Phase::Travail)) };
            if icons::wide(ui, Icon::Clock, &label, "Ctrl+P", ui.available_width(), if running { Some((p.live, p.on_accent)) } else { None }, &p).clicked() {
                acts.push(Act::Pomo(id));
            }
            let spent = self.data.tasks[ix].spent;
            if spent > 0 {
                ui.label(RichText::new(format!("Temps passé : {}", fmt_spent(spent))).size(theme::taille::META).color(p.muted));
            }
            if running {
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
        }
        ui.add_space(6.0);
        // liste
        {
            let cur = self.data.tasks[ix].list;
            let mut pick = cur;
            egui::ComboBox::from_id_salt("d-liste").selected_text(self.data.list_name(cur).to_string()).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
                for l in &self.data.lists {
                    ui.selectable_value(&mut pick, l.id, &l.name);
                }
            });
            if pick != cur {
                acts.push(Act::MoveTo(id, pick));
            }
        }
        ui.add_space(6.0);
        // note
        ui.label(RichText::new("NOTE").size(theme::taille::ENTETE).color(p.muted));
        {
            let t = &mut self.data.tasks[ix];
            changed |= ui.add(egui::TextEdit::multiline(&mut t.note).desired_rows(4).desired_width(ui.available_width()).hint_text("Ajouter une note")).changed();
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let t = &self.data.tasks[ix];
            ui.label(RichText::new(format!("Créée {}", nlp::describe(t.created.date(), None, today))).size(theme::taille::META).color(p.muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icons::button(ui, Icon::Trash, 28.0, Look { fg: p.del, ..Look::tool(&p) }, "Supprimer (Suppr)").clicked() {
                    acts.push(Act::Delete(id));
                }
            });
        });
        if let Some(s) = sound.take() {
            self.ding(s);
        }
        if changed {
            self.data.tasks[ix].touched = n;
            self.touch();
        }
    }

    fn settings_ui(&mut self, ctx: &egui::Context) {
        let p = self.pal.clone();
        let mut s = self.data.settings.clone();
        let mut dark_pick: Option<Option<bool>> = None;
        let m = socle::ui::modal(ctx, "reglages", &p, 440.0, 70.0, |ui| {
            ui.label(RichText::new("Réglages").size(theme::taille::TITRE).color(p.text));
            ui.add_space(8.0);
            let mut cur = match s.sombre {
                None => 0,
                Some(false) => 1,
                Some(true) => 2,
            };
            ui.horizontal(|ui| {
                ui.label("Thème");
                if socle::ui::segmented(ui, &p, &mut cur, &[(None, "Système"), (None, "Clair"), (None, "Sombre")]) {
                    dark_pick = Some([None, Some(false), Some(true)][cur]);
                }
            });
            ui.add_space(6.0);
            ui.checkbox(&mut s.sons, "Sons (ding, paliers, fin de pomodoro)");
            ui.horizontal(|ui| {
                ui.label("Volume");
                let mut v = (s.volume * 100.0).round() as i32;
                if ui.add(egui::Slider::new(&mut v, 0..=100).suffix(" %")).changed() {
                    s.volume = v as f32 / 100.0;
                }
                if ui.button("Essayer").clicked() && s.sons {
                    sound::play(Sound::Done, s.volume);
                }
            });
            ui.add_space(6.0);
            ui.label(RichText::new("POMODORO (MINUTES)").size(theme::taille::ENTETE).color(p.muted));
            egui::Grid::new("pomo").num_columns(2).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
                ui.label("Travail");
                ui.add(egui::DragValue::new(&mut s.pomo_travail).range(1..=240));
                ui.end_row();
                ui.label("Pause");
                ui.add(egui::DragValue::new(&mut s.pomo_pause).range(1..=120));
                ui.end_row();
                ui.label("Longue pause");
                ui.add(egui::DragValue::new(&mut s.pomo_longue).range(1..=120));
                ui.end_row();
                ui.label("Longue pause tous les");
                ui.add(egui::DragValue::new(&mut s.pomo_cycles).range(1..=12).suffix(" cycles"));
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.checkbox(&mut s.notifications, "Notifications de rappel (discrètes, sans son)");
            ui.checkbox(&mut s.suggestions_matin, "Proposer les suggestions de Ma journée chaque matin");
            ui.checkbox(&mut s.demarrage_windows, "Démarrer avec Windows (dans la zone de notification)");
            ui.add_space(8.0);
            ui.label(RichText::new(format!("Tâches {} · socle {} · données : {}", env!("CARGO_PKG_VERSION"), socle::VERSION, self.path.display())).size(theme::taille::META).color(p.muted));
            ui.add_space(4.0);
            ui.button("Fermer").clicked()
        });
        if s != self.data.settings {
            if s.demarrage_windows != self.data.settings.demarrage_windows && !socle::testmode::enabled() {
                tray::autostart(s.demarrage_windows);
            }
            self.data.settings = s;
            self.touch();
        }
        if let Some(d) = dark_pick {
            self.set_dark(ctx, d);
        }
        if m.close || m.inner {
            self.settings_open = false;
        }
    }

    fn confirm_list_ui(&mut self, ctx: &egui::Context, id: u64) {
        let p = self.pal.clone();
        let name = self.data.list_name(id).to_string();
        let k = self.data.tasks.iter().filter(|t| t.list == id).count();
        let mut go = false;
        let m = socle::ui::modal(ctx, "suppr-liste", &p, 380.0, 120.0, |ui| {
            ui.label(RichText::new(format!("Supprimer la liste « {name} » ?")).size(theme::taille::TITRE).color(p.text));
            ui.label(RichText::new(format!("{k} tâche(s) seront supprimées définitivement.")).color(p.muted));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Supprimer").color(p.del)).clicked() {
                    go = true;
                }
                ui.button("Annuler").clicked()
            })
            .inner
        });
        if go {
            self.data.remove_list(id);
            if self.view == View::List(id) {
                self.set_view(View::List(DEFAULT_LIST));
            }
            self.touch();
            self.confirm_delete_list = None;
        } else if m.close || m.inner {
            self.confirm_delete_list = None;
        }
    }
}

/// Curseur fixe : sans clignotement, aucun réveil au repos (CPU ~0 %).
fn no_blink(ctx: &egui::Context) {
    ctx.all_styles_mut(|s| s.visuals.text_cursor.blink = false);
}

/// « mercredi 25 novembre »
fn long_date(d: NaiveDate) -> String {
    use chrono::Datelike;
    const J: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];
    const M: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
    format!("{} {} {}", J[d.weekday().num_days_from_monday() as usize], d.day(), M[d.month0() as usize])
}

fn nine() -> NaiveTime {
    NaiveTime::from_hms_opt(9, 0, 0).unwrap_or_default()
}

fn k_mods_plain(m: Modifiers) -> bool {
    !m.ctrl && !m.alt && !m.command
}

pub fn fmt_spent(secs: u64) -> String {
    let m = secs / 60;
    if m >= 60 { format!("{} h {:02}", m / 60, m % 60) } else { format!("{m} min") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> App {
        let dir = std::env::temp_dir().join(format!("taches-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        App::new(&egui::Context::default(), dir.join("taches.json"))
    }

    #[test]
    fn ajout_naturel_et_enregistrement() {
        let mut a = app("ajout");
        let id = a.add_parsed("beurre demain 18h #courses !", View::MyDay).unwrap();
        let t = a.data.get(id).unwrap().clone();
        assert_eq!(t.title, "beurre");
        assert!(t.important && t.due.is_some() && t.remind.is_some());
        assert_eq!(a.data.list_name(t.list), "courses");
        assert!(t.in_my_day(a.today), "ajoutée depuis Ma journée");
        let id2 = a.add_parsed("pompes x8", View::Important).unwrap();
        assert_eq!(a.data.get(id2).unwrap().target, 8);
        assert!(a.data.get(id2).unwrap().important);
        assert!(a.add_parsed("   ", View::MyDay).is_none());
        a.flush();
        let back: Data = socle::settings::load_from(&a.path);
        assert_eq!(back.tasks.len(), 2);
        assert_eq!(back, a.data);
    }

    #[test]
    fn pomodoro_compte_le_temps() {
        let mut a = app("pomo");
        let id = a.add_parsed("écrire", View::MyDay).unwrap();
        a.pomo_start(Some(id));
        a.pomo.as_mut().unwrap().end = Instant::now();
        a.pomo_next(false);
        assert_eq!(a.data.get(id).unwrap().spent, 25 * 60);
        assert_eq!(a.pomo.as_ref().unwrap().phase, Phase::Pause);
        a.pomo_next(true);
        assert_eq!(a.pomo.as_ref().unwrap().phase, Phase::Travail);
        assert_eq!(a.data.get(id).unwrap().spent, 25 * 60, "passer une pause ne compte rien");
        a.data.settings.pomo_cycles = 2;
        a.pomo.as_mut().unwrap().end = Instant::now();
        a.pomo_next(false);
        assert_eq!(a.pomo.as_ref().unwrap().phase, Phase::Longue, "longue pause au 2e cycle");
        a.pomodoro_toggle();
        assert!(a.pomo.is_none());
        assert!(a.pomo_label().is_none());
        // session libre, pause puis reprise : le temps restant ne bouge pas en pause
        a.pomo_task = None;
        a.pomo_play_pause();
        assert!(a.pomo.as_ref().unwrap().running() && a.pomo.as_ref().unwrap().task.is_none());
        a.pomo_play_pause();
        let left = a.pomo.as_ref().unwrap().left();
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(a.pomo.as_ref().unwrap().left(), left);
        assert!(a.pomo_label().unwrap().contains("en pause"));
        a.pomo.as_mut().unwrap().paused = Some(Duration::from_secs(20 * 60));
        a.pomo_stop();
        assert_eq!(a.data.free_spent, 5 * 60, "5 min de travail en session libre");
        assert_eq!(a.data.get(id).unwrap().spent, 50 * 60);
    }

    #[test]
    fn supprimer_et_annuler() {
        let mut a = app("annuler");
        let id = a.add_parsed("x", View::MyDay).unwrap();
        a.apply(Act::Delete(id));
        assert!(a.data.get(id).is_none());
        a.undo();
        assert!(a.data.get(id).is_some());
        a.apply(Act::Toggle(id));
        assert!(a.data.get(id).unwrap().is_done());
        a.undo();
        assert!(!a.data.get(id).unwrap().is_done());
    }

    /// Une image complète de l'interface, sans fenêtre : pas de panique, rendu rapide.
    #[test]
    fn rendu_sans_fenetre() {
        let mut a = app("rendu");
        for i in 0..300 {
            a.add_parsed(&format!("tâche {i} demain #liste{}", i % 5), View::MyDay);
        }
        let id = a.data.tasks[3].id;
        a.data.get_mut(id).unwrap().steps = vec![Step { title: "a".into(), done: true }, Step { title: "b".into(), done: false }];
        a.data.get_mut(id).unwrap().sequential = true;
        a.sel = Some(id);
        let ctx = egui::Context::default();
        let input = || egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0))), ..Default::default() };
        a.pomo_start(Some(id));
        for v in [View::MyDay, View::Planned, View::Important, View::List(DEFAULT_LIST), View::Search, View::Pomodoro] {
            a.view = v;
            a.query = "tâche 1".into();
            let _ = ctx.run_ui(input(), |ui| a.frame(ui));
        }
        a.view = View::MyDay;
        a.suggest_open = true;
        let t0 = Instant::now();
        let _ = ctx.run_ui(input(), |ui| a.frame(ui));
        let ms = t0.elapsed().as_millis();
        a.settings_open = true;
        let _ = ctx.run_ui(input(), |ui| a.frame(ui));
        eprintln!("image avec 300 tâches : {ms} ms");
        assert!(ms < 200);
    }
}
