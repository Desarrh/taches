//! Données et règles : listes, tâches, compteurs, Ma journée, suggestions. Sans interface.

use crate::sound::Sound;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

/// Liste par défaut « Tâches » : jamais supprimée.
pub const DEFAULT_LIST: u64 = 1;
/// Une tâche terminée reste à sa place (et annulable) pendant ce délai.
pub const UNDO_SECS: i64 = 5;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Reset {
    #[default]
    Jamais,
    Jour,
    Semaine,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Step {
    pub title: String,
    pub done: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Task {
    pub id: u64,
    pub list: u64,
    pub title: String,
    pub note: String,
    pub important: bool,
    pub due: Option<NaiveDate>,
    pub remind: Option<NaiveDateTime>,
    /// rappel déjà notifié
    pub reminded: bool,
    /// dans Ma journée ce jour-là (vidée d'elle-même le lendemain)
    pub my_day: Option<NaiveDate>,
    pub steps: Vec<Step>,
    /// étapes débloquées une à une
    pub sequential: bool,
    /// tâche à compteur : 0 = non
    pub target: u32,
    pub count: u32,
    pub reset: Reset,
    /// début de la période en cours du compteur
    pub period: Option<NaiveDate>,
    pub done_at: Option<NaiveDateTime>,
    pub created: NaiveDateTime,
    pub touched: NaiveDateTime,
    /// secondes de pomodoro (travail)
    pub spent: u64,
    /// ordre manuel dans la liste (plus grand = plus haut)
    pub order: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct List {
    pub id: u64,
    pub name: String,
    /// nom d'icône (voir `app::LIST_ICONS`)
    pub icon: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// None = suivre Windows
    pub sombre: Option<bool>,
    pub sons: bool,
    /// 0..=1
    pub volume: f32,
    pub pomo_travail: u32,
    pub pomo_pause: u32,
    pub pomo_longue: u32,
    pub pomo_cycles: u32,
    pub notifications: bool,
    pub demarrage_windows: bool,
    pub suggestions_matin: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { sombre: None, sons: true, volume: 0.6, pomo_travail: 25, pomo_pause: 5, pomo_longue: 15, pomo_cycles: 4, notifications: true, demarrage_windows: true, suggestions_matin: true }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Data {
    pub version: u32,
    pub next_id: u64,
    /// ordre d'affichage dans la barre latérale
    pub lists: Vec<List>,
    pub tasks: Vec<Task>,
    pub settings: Settings,
    /// dernier jour où les suggestions du matin ont été proposées
    pub suggested_on: Option<NaiveDate>,
}

impl Default for Data {
    fn default() -> Self {
        Data { version: 1, next_id: 2, lists: vec![List { id: DEFAULT_LIST, name: "Tâches".into(), icon: "maison".into() }], tasks: Vec::new(), settings: Settings::default(), suggested_on: None }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Why {
    EnRetard,
    Aujourdhui,
    Recente,
    Oubliee,
}

impl Why {
    pub fn label(self) -> &'static str {
        match self {
            Why::EnRetard => "En retard",
            Why::Aujourdhui => "À échéance aujourd'hui",
            Why::Recente => "Ajoutées récemment",
            Why::Oubliee => "Oubliées depuis longtemps",
        }
    }
}

pub fn week_start(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

impl Task {
    pub fn is_done(&self) -> bool {
        self.done_at.is_some()
    }
    /// Terminée depuis plus de 5 s : elle descend dans « Terminées ».
    pub fn settled(&self, now: NaiveDateTime) -> bool {
        self.done_at.is_some_and(|d| (now - d).num_seconds() >= UNDO_SECS)
    }
    pub fn in_my_day(&self, today: NaiveDate) -> bool {
        self.my_day == Some(today)
    }
    pub fn steps_done(&self) -> usize {
        self.steps.iter().filter(|s| s.done).count()
    }
    /// Index de la prochaine étape à faire (mode une à une).
    pub fn next_step(&self) -> Option<usize> {
        self.steps.iter().position(|s| !s.done)
    }
}

impl Data {
    pub fn get(&self, id: u64) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }
    pub fn get_mut(&mut self, id: u64) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }
    pub fn list(&self, id: u64) -> Option<&List> {
        self.lists.iter().find(|l| l.id == id)
    }
    pub fn list_name(&self, id: u64) -> &str {
        self.list(id).map_or("Tâches", |l| l.name.as_str())
    }
    fn fresh_id(&mut self) -> u64 {
        let id = self.next_id.max(2);
        self.next_id = id + 1;
        id
    }

    pub fn add_list(&mut self, name: &str) -> u64 {
        let id = self.fresh_id();
        self.lists.push(List { id, name: name.trim().to_string(), icon: "liste".into() });
        id
    }

    /// Liste par nom (insensible aux accents et à la casse), créée au besoin.
    pub fn list_by_name(&mut self, name: &str) -> u64 {
        let f = socle::fuzzy::fold(name);
        if let Some(l) = self.lists.iter().find(|l| socle::fuzzy::fold(&l.name) == f) {
            return l.id;
        }
        self.add_list(name)
    }

    /// Supprime une liste et ses tâches (jamais la liste par défaut).
    pub fn remove_list(&mut self, id: u64) -> bool {
        if id == DEFAULT_LIST {
            return false;
        }
        self.lists.retain(|l| l.id != id);
        self.tasks.retain(|t| t.list != id);
        true
    }

    pub fn add_task(&mut self, list: u64, title: &str, now: NaiveDateTime) -> u64 {
        let id = self.fresh_id();
        let order = self.tasks.iter().filter(|t| t.list == list).map(|t| t.order).max().unwrap_or(0) + 1;
        self.tasks.push(Task { id, list, title: title.trim().to_string(), created: now, touched: now, order, ..Default::default() });
        id
    }

    /// Coche ou décoche (ou fait avancer un compteur). Renvoie le son à jouer.
    pub fn toggle(&mut self, id: u64, now: NaiveDateTime) -> Option<Sound> {
        let t = self.get_mut(id)?;
        t.touched = now;
        if t.is_done() {
            t.done_at = None;
            if t.target > 0 && t.count >= t.target {
                t.count = t.target - 1;
            }
            return None;
        }
        if t.target > 0 {
            t.count += 1;
            if t.count >= t.target {
                t.count = t.target;
                t.done_at = Some(now);
                return Some(Sound::Final);
            }
            return Some(Sound::Step(t.count, t.target));
        }
        t.done_at = Some(now);
        Some(Sound::Done)
    }

    /// Recule d'un cran un compteur (annuler un appui en trop).
    pub fn count_back(&mut self, id: u64, now: NaiveDateTime) {
        if let Some(t) = self.get_mut(id) {
            if t.target > 0 && t.count > 0 {
                t.count -= 1;
                t.done_at = None;
                t.touched = now;
            }
        }
    }

    /// Coche ou décoche une étape. En mode une à une, la dernière étape termine la tâche.
    pub fn toggle_step(&mut self, id: u64, i: usize, now: NaiveDateTime) -> Option<Sound> {
        let t = self.get_mut(id)?;
        let n = t.steps.len() as u32;
        let s = t.steps.get_mut(i)?;
        s.done = !s.done;
        let done = s.done;
        t.touched = now;
        if !done {
            if t.sequential {
                t.done_at = None;
            }
            return None;
        }
        let k = t.steps_done() as u32;
        if k == n && t.sequential {
            t.done_at = Some(now);
            return Some(Sound::Final);
        }
        Some(Sound::Step(k, n))
    }

    /// Changement de jour ou de semaine : les compteurs à remise à zéro repartent.
    pub fn roll(&mut self, today: NaiveDate) {
        for t in &mut self.tasks {
            let start = match t.reset {
                Reset::Jamais => continue,
                Reset::Jour => today,
                Reset::Semaine => week_start(today),
            };
            if t.period != Some(start) {
                if t.period.is_some() {
                    t.count = 0;
                    t.done_at = None;
                }
                t.period = Some(start);
            }
        }
    }

    /// Rappels arrivés à échéance et pas encore notifiés (marqués notifiés).
    pub fn due_reminders(&mut self, now: NaiveDateTime) -> Vec<(u64, String)> {
        let mut out = Vec::new();
        for t in &mut self.tasks {
            if !t.reminded && !t.is_done() && t.remind.is_some_and(|r| r <= now) {
                t.reminded = true;
                out.push((t.id, t.title.clone()));
            }
        }
        out
    }

    /// Prochain rappel à venir (pour réveiller la boucle juste à temps).
    pub fn next_reminder(&self) -> Option<NaiveDateTime> {
        self.tasks.iter().filter(|t| !t.reminded && !t.is_done()).filter_map(|t| t.remind).min()
    }

    /// Suggestions pour Ma journée, comme le panneau de To Do.
    pub fn suggestions(&self, now: NaiveDateTime) -> Vec<(u64, Why)> {
        let today = now.date();
        let mut out: Vec<(u64, Why, i64)> = Vec::new();
        for t in self.tasks.iter().filter(|t| !t.is_done() && !t.in_my_day(today)) {
            let why = match t.due {
                Some(d) if d < today => Some(Why::EnRetard),
                Some(d) if d == today => Some(Why::Aujourdhui),
                _ if t.remind.is_some_and(|r| r.date() == today) => Some(Why::Aujourdhui),
                _ if now - t.created < Duration::days(2) => Some(Why::Recente),
                _ if now - t.touched > Duration::days(14) => Some(Why::Oubliee),
                _ => None,
            };
            if let Some(w) = why {
                let key = match w {
                    Why::EnRetard => t.due.map_or(0, |d| d.num_days_from_ce() as i64),
                    Why::Oubliee => t.touched.and_utc().timestamp(),
                    _ => -t.created.and_utc().timestamp(),
                };
                out.push((t.id, w, key));
            }
        }
        out.sort_by_key(|&(_, w, k)| (w as u8, k));
        out.truncate(20);
        out.into_iter().map(|(id, w, _)| (id, w)).collect()
    }

    /// Déplace `id` juste avant `before` dans sa liste (ordre manuel).
    pub fn reorder(&mut self, id: u64, before: u64) {
        let Some(list) = self.get(before).map(|t| t.list) else { return };
        if let Some(t) = self.get_mut(id) {
            t.list = list;
        }
        let mut ids: Vec<(i64, u64)> = self.tasks.iter().filter(|t| t.list == list && t.id != id).map(|t| (t.order, t.id)).collect();
        ids.sort_by(|a, b| b.cmp(a));
        let mut seq: Vec<u64> = ids.into_iter().map(|x| x.1).collect();
        let at = seq.iter().position(|&x| x == before).unwrap_or(0);
        seq.insert(at, id);
        let n = seq.len() as i64;
        for (i, tid) in seq.into_iter().enumerate() {
            if let Some(t) = self.get_mut(tid) {
                t.order = n - i as i64;
            }
        }
    }

    /// Déplace la liste `id` juste avant `before` dans la barre latérale.
    pub fn move_list(&mut self, id: u64, before: u64) {
        let Some(from) = self.lists.iter().position(|l| l.id == id) else { return };
        let l = self.lists.remove(from);
        let at = self.lists.iter().position(|l| l.id == before).unwrap_or(self.lists.len());
        self.lists.insert(at, l);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap().and_hms_opt(h, 0, 0).unwrap()
    }

    #[test]
    fn cocher_et_annuler() {
        let mut d = Data::default();
        let id = d.add_task(DEFAULT_LIST, "  pain ", at(25, 10));
        assert_eq!(d.get(id).unwrap().title, "pain");
        assert_eq!(d.toggle(id, at(25, 10)), Some(Sound::Done));
        assert!(!d.get(id).unwrap().settled(at(25, 10)));
        assert!(d.get(id).unwrap().settled(at(25, 11)));
        assert_eq!(d.toggle(id, at(25, 10)), None);
        assert!(!d.get(id).unwrap().is_done());
    }

    #[test]
    fn compteur_et_remise_a_zero() {
        let mut d = Data::default();
        let id = d.add_task(DEFAULT_LIST, "pompes", at(25, 10));
        d.get_mut(id).unwrap().target = 3;
        d.get_mut(id).unwrap().reset = Reset::Jour;
        d.roll(at(25, 10).date());
        assert_eq!(d.toggle(id, at(25, 10)), Some(Sound::Step(1, 3)));
        assert_eq!(d.toggle(id, at(25, 10)), Some(Sound::Step(2, 3)));
        assert_eq!(d.toggle(id, at(25, 10)), Some(Sound::Final));
        assert!(d.get(id).unwrap().is_done());
        // décocher revient à 2/3
        d.toggle(id, at(25, 10));
        assert_eq!(d.get(id).unwrap().count, 2);
        d.toggle(id, at(25, 10));
        d.roll(at(25, 23).date());
        assert!(d.get(id).unwrap().is_done(), "même jour : rien ne bouge");
        d.roll(at(26, 7).date());
        let t = d.get(id).unwrap();
        assert_eq!((t.count, t.is_done()), (0, false), "nouveau jour : compteur à zéro");
        // semaine : du mercredi au dimanche rien, lundi remise
        let w = d.add_task(DEFAULT_LIST, "lessive", at(25, 10));
        d.get_mut(w).unwrap().target = 2;
        d.get_mut(w).unwrap().reset = Reset::Semaine;
        d.roll(at(25, 10).date());
        d.toggle(w, at(25, 10));
        d.roll(at(29, 10).date());
        assert_eq!(d.get(w).unwrap().count, 1);
        d.roll(at(30, 10).date());
        assert_eq!(d.get(w).unwrap().count, 0);
    }

    #[test]
    fn etapes_une_a_une() {
        let mut d = Data::default();
        let id = d.add_task(DEFAULT_LIST, "déménager", at(25, 10));
        {
            let t = d.get_mut(id).unwrap();
            t.sequential = true;
            t.steps = ["cartons", "camion", "clés"].iter().map(|s| Step { title: s.to_string(), done: false }).collect();
        }
        assert_eq!(d.get(id).unwrap().next_step(), Some(0));
        assert_eq!(d.toggle_step(id, 0, at(25, 10)), Some(Sound::Step(1, 3)));
        assert_eq!(d.get(id).unwrap().next_step(), Some(1));
        d.toggle_step(id, 1, at(25, 10));
        assert_eq!(d.toggle_step(id, 2, at(25, 10)), Some(Sound::Final));
        assert!(d.get(id).unwrap().is_done());
        d.toggle_step(id, 2, at(25, 10));
        assert!(!d.get(id).unwrap().is_done());
    }

    #[test]
    fn ma_journee_et_suggestions() {
        let mut d = Data::default();
        let now = at(25, 8);
        let late = d.add_task(DEFAULT_LIST, "en retard", at(1, 8));
        d.get_mut(late).unwrap().due = Some(at(20, 0).date());
        let today = d.add_task(DEFAULT_LIST, "aujourd'hui", at(1, 8));
        d.get_mut(today).unwrap().due = Some(now.date());
        let recent = d.add_task(DEFAULT_LIST, "récente", at(24, 20));
        let old = d.add_task(DEFAULT_LIST, "oubliée", at(1, 8));
        let quiet = d.add_task(DEFAULT_LIST, "rien", at(20, 8));
        let mine = d.add_task(DEFAULT_LIST, "déjà", at(24, 20));
        d.get_mut(mine).unwrap().my_day = Some(now.date());
        let s = d.suggestions(now);
        let ids: Vec<u64> = s.iter().map(|x| x.0).collect();
        assert_eq!(ids, vec![late, today, recent, old]);
        assert!(!ids.contains(&quiet) && !ids.contains(&mine));
        // Ma journée se vide le lendemain
        assert!(d.get(mine).unwrap().in_my_day(now.date()));
        assert!(!d.get(mine).unwrap().in_my_day(at(26, 8).date()));
    }

    #[test]
    fn rappels() {
        let mut d = Data::default();
        let id = d.add_task(DEFAULT_LIST, "appeler", at(25, 8));
        d.get_mut(id).unwrap().remind = Some(at(25, 18));
        assert_eq!(d.next_reminder(), Some(at(25, 18)));
        assert!(d.due_reminders(at(25, 17)).is_empty());
        assert_eq!(d.due_reminders(at(25, 18)).len(), 1);
        assert!(d.due_reminders(at(25, 19)).is_empty(), "une seule fois");
        assert_eq!(d.next_reminder(), None);
    }

    #[test]
    fn ordre_et_listes() {
        let mut d = Data::default();
        let a = d.add_task(DEFAULT_LIST, "a", at(25, 8));
        let b = d.add_task(DEFAULT_LIST, "b", at(25, 8));
        let c = d.add_task(DEFAULT_LIST, "c", at(25, 8));
        d.reorder(a, c); // a passe en tête
        let mut v: Vec<&Task> = d.tasks.iter().collect();
        v.sort_by_key(|t| -t.order);
        assert_eq!(v.iter().map(|t| t.id).collect::<Vec<_>>(), vec![a, c, b]);
        let courses = d.list_by_name("Courses");
        assert_eq!(d.list_by_name("courses"), courses);
        d.move_list(courses, DEFAULT_LIST);
        assert_eq!(d.lists[0].id, courses);
        assert!(!d.remove_list(DEFAULT_LIST));
        d.add_task(courses, "beurre", at(25, 8));
        assert!(d.remove_list(courses));
        assert_eq!(d.tasks.len(), 3);
        // relecture tolérante : champs manquants = défaut
        let j: Data = serde_json::from_str(r#"{"tasks":[{"id":9,"title":"x"}]}"#).unwrap();
        assert_eq!(j.tasks[0].list, 0);
        assert!(j.settings.sons);
    }
}
