//! Commandes sans fenêtre pour le Lanceur (marqueur `TachesCli` de la ressource de version).
//! - `--comprendre "<texte>" --json` : parseur seul, sans charger les données ;
//! - `--lister [ma-journee|tout] --json [--chercher "<texte>"] [--max 20]` : lecture seule ;
//! - `--ajouter "<texte>"`, `--cocher <id>` : transmis à l'instance qui tourne, sinon écrits
//!   directement dans le fichier. Aucune fenêtre n'est ouverte ni ramenée.

use crate::model::{DEFAULT_LIST, Data, Task};
use crate::{app, nlp, sound};
use chrono::{NaiveDate, NaiveDateTime};
use serde_json::{Value, json};
use std::path::Path;

fn arg<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

/// « AAAA-MM-JJ HH:MM » quand il y a une heure (rappel le même jour), sinon « AAAA-MM-JJ ».
fn when(due: Option<NaiveDate>, remind: Option<NaiveDateTime>) -> Value {
    match (due, remind) {
        (d, Some(r)) if d.is_none_or(|d| d == r.date()) => json!(r.format("%Y-%m-%d %H:%M").to_string()),
        (Some(d), _) => json!(d.format("%Y-%m-%d").to_string()),
        _ => Value::Null,
    }
}

pub fn comprendre(text: &str, now: NaiveDateTime) -> String {
    let p = nlp::parse(text, now);
    json!({"v": 1, "titre": p.title, "liste": p.list, "echeance": when(p.due, p.remind), "important": p.important, "compteur": p.target}).to_string()
}

/// Lecture sans effet de bord (pas de copie `.bad`).
fn load(path: &Path) -> Data {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn lister(d: &Data, which: &str, query: Option<&str>, max: usize, today: NaiveDate) -> String {
    let mut open: Vec<&Task> = d.tasks.iter().filter(|t| !t.is_done() && (which != "ma-journee" || t.in_my_day(today))).collect();
    open.sort_by_key(|t| (!t.in_my_day(today), !t.important, -t.order));
    let picked: Vec<&Task> = match query.map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => {
            let mut idx = socle::fuzzy::Index::new();
            for t in &open {
                idx.push(&format!("{} {}", t.title, t.note), if t.in_my_day(today) { 20 } else { 0 });
            }
            idx.search(q, max).into_iter().filter_map(|h| open.get(h.index).copied()).collect()
        }
        None => open.into_iter().take(max).collect(),
    };
    let taches: Vec<Value> = picked
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "titre": t.title,
                "liste": d.list_name(t.list),
                "echeance": when(t.due, t.remind),
                "important": t.important,
                "compteur": (t.target > 0).then(|| format!("{}/{}", t.count, t.target)),
            })
        })
        .collect();
    json!({"v": 1, "taches": taches}).to_string()
}

/// Ajout ou coche directs dans le fichier (aucune instance ne tourne). Code de sortie.
fn direct(args: &[String], path: &Path) -> i32 {
    let now = app::now();
    let mut d = load(path);
    d.roll(now.date());
    if let Some(text) = arg(args, "--ajouter") {
        if d.add_text(text, DEFAULT_LIST, now).is_none() {
            return 2;
        }
    } else if let Some(id) = arg(args, "--cocher").and_then(|v| v.parse::<u64>().ok()) {
        if !d.get(id).is_some_and(|t| !t.is_done()) {
            return 2;
        }
        if let Some(s) = d.toggle(id, now) {
            if d.settings.sons && !sound::MUTE.load(std::sync::atomic::Ordering::Relaxed) {
                sound::play(s, d.settings.volume);
                // laisser le son finir avant la sortie du processus
                std::thread::sleep(std::time::Duration::from_millis(750));
            }
        }
    }
    match serde_json::to_vec_pretty(&d).map_err(|e| e.to_string()).and_then(|j| socle::paths::write_atomic(path, &j)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            2
        }
    }
}

/// Some(code) si les arguments sont une commande sans fenêtre.
pub fn run(args: &[String], path: &Path) -> Option<i32> {
    let has = |f: &str| args.iter().any(|a| a == f);
    let now = app::now();
    if has("--comprendre") {
        println!("{}", comprendre(arg(args, "--comprendre").unwrap_or(""), now));
        return Some(0);
    }
    if has("--lister") {
        let which = arg(args, "--lister").filter(|w| !w.starts_with("--")).unwrap_or("tout");
        let max = arg(args, "--max").and_then(|v| v.parse().ok()).unwrap_or(20usize);
        println!("{}", lister(&load(path), which, arg(args, "--chercher"), max, now.date()));
        return Some(0);
    }
    if !has("--ajouter") && !has("--cocher") {
        return None;
    }
    // vérifications avant transmission (l'instance ne renvoie pas de résultat)
    if let Some(text) = arg(args, "--ajouter") {
        if nlp::parse(text, now).title.trim().is_empty() {
            return Some(2);
        }
    } else {
        let Some(id) = arg(args, "--cocher").and_then(|v| v.parse::<u64>().ok()) else { return Some(2) };
        if !load(path).get(id).is_some_and(|t| !t.is_done()) {
            return Some(2);
        }
    }
    if socle::testmode::enabled() {
        return Some(direct(args, path));
    }
    Some(match socle::instance::start("Taches", args) {
        socle::instance::Start::Forwarded => 0,
        // pas d'instance : on garde le verrou le temps d'écrire, sans ouvrir de fenêtre
        socle::instance::Start::Primary(_i) => direct(args, path),
        socle::instance::Start::Alone => direct(args, path),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 25).unwrap().and_hms_opt(h, 0, 0).unwrap()
    }

    #[test]
    fn contrat_json() {
        let v: Value = serde_json::from_str(&comprendre("beurre demain 18h #courses !", at(10))).unwrap();
        assert_eq!(v["v"], 1);
        assert_eq!(v["titre"], "beurre");
        assert_eq!(v["liste"], "courses");
        assert_eq!(v["echeance"], "2026-11-26 18:00");
        assert_eq!(v["important"], true);
        assert_eq!(v["compteur"], Value::Null);
        let v: Value = serde_json::from_str(&comprendre("pompes x8", at(10))).unwrap();
        assert_eq!((v["echeance"].clone(), v["compteur"].clone(), v["liste"].clone()), (Value::Null, json!(8), Value::Null));
        let v: Value = serde_json::from_str(&comprendre("rapport lundi", at(10))).unwrap();
        assert_eq!(v["echeance"], "2026-11-30");

        let mut d = Data::default();
        let a = d.add_text("pain #courses", DEFAULT_LIST, at(9)).unwrap();
        let b = d.add_text("pompes x8", DEFAULT_LIST, at(9)).unwrap();
        let c = d.add_text("fini", DEFAULT_LIST, at(9)).unwrap();
        d.get_mut(b).unwrap().my_day = Some(at(9).date());
        d.toggle(b, at(9));
        d.toggle(c, at(9));
        let v: Value = serde_json::from_str(&lister(&d, "tout", None, 20, at(9).date())).unwrap();
        let ids: Vec<u64> = v["taches"].as_array().unwrap().iter().map(|t| t["id"].as_u64().unwrap()).collect();
        assert_eq!(ids, vec![b, a], "Ma journée d'abord, terminées exclues");
        assert_eq!(v["taches"][0]["compteur"], "1/8");
        assert_eq!(v["taches"][1]["liste"], "courses");
        let v: Value = serde_json::from_str(&lister(&d, "ma-journee", None, 20, at(9).date())).unwrap();
        assert_eq!(v["taches"].as_array().unwrap().len(), 1);
        let v: Value = serde_json::from_str(&lister(&d, "tout", Some("pain"), 20, at(9).date())).unwrap();
        assert_eq!(v["taches"][0]["id"], a);
    }

    #[test]
    fn ajouter_et_cocher_sans_instance() {
        let dir = std::env::temp_dir().join(format!("taches-cli-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("taches.json");
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(direct(&s(&["--ajouter", "lait #courses"]), &path), 0);
        assert_eq!(direct(&s(&["--ajouter", "   "]), &path), 2);
        let d = load(&path);
        let id = d.tasks[0].id;
        assert_eq!(direct(&s(&["--cocher", &id.to_string()]), &path), 0);
        assert!(load(&path).get(id).unwrap().is_done());
        assert_eq!(direct(&s(&["--cocher", &id.to_string()]), &path), 2, "déjà cochée");
        assert_eq!(direct(&s(&["--cocher", "999"]), &path), 2);
        assert_eq!(run(&s(&["--cocher", "abc"]), &path), Some(2));
        assert_eq!(run(&s(&["--rien"]), &path), None);
    }
}
