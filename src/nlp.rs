//! Saisie naturelle en français : "beurre demain 18h #courses !" -> titre, échéance, rappel, liste.
//! Pas de regex : on découpe en mots, on essaie de reconnaître des groupes de mots à chaque position.
use chrono::{Datelike, Days, Months, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Date,
    Time,
    List,
    Important,
    Count,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Parsed {
    pub title: String,                 // saisie sans les jetons reconnus
    pub due: Option<NaiveDate>,
    pub remind: Option<NaiveDateTime>, // seulement si une heure a été donnée
    pub list: Option<String>,
    pub important: bool,
    pub target: Option<u32>,           // tâche à compteur : x8, 8x, 8 fois
    pub spans: Vec<(Range<usize>, Kind)>, // plages d'octets reconnues, triées
}

// Un mot : texte original, offset d'octet, forme normalisée (minuscules, sans accents).
struct Tok<'a> {
    o: &'a str,
    s: usize,
    n: String,
}

// Ce qu'un groupe de mots reconnu apporte.
enum Hit {
    Date(NaiveDate),
    Time(NaiveTime, bool), // bool : « ce soir » force aujourd'hui
    Rel(NaiveDateTime),    // « dans 2 heures »
    List(String),
    Important,
    Count(u32),
}

fn norm(w: &str) -> String {
    let s: String = w
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'à' | 'â' | 'ä' => 'a',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            '’' => '\'',
            c => c,
        })
        .collect();
    s.trim_end_matches(['.', ',', ';']).to_string()
}

// Nombre décimal pur de au plus `max` chiffres.
fn num(s: &str, max: usize) -> Option<u32> {
    if s.is_empty() || s.len() > max || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

// Compteur valide : 2..=999.
fn cnt(s: &str) -> Option<u32> {
    num(s, 3).filter(|n| (2..=999).contains(n))
}

// "18h", "18h30", "18:30".
fn hm(s: &str) -> Option<NaiveTime> {
    let (h, m) = s.split_once(':').or_else(|| s.split_once('h'))?;
    let m = if m.is_empty() {
        if s.contains(':') {
            return None;
        }
        0
    } else if m.len() == 2 {
        num(m, 2)?
    } else {
        return None;
    };
    NaiveTime::from_hms_opt(num(h, 2)?, m, 0)
}

fn month(w: &str) -> Option<u32> {
    Some(match w {
        "janvier" | "janv" => 1,
        "fevrier" | "fevr" | "fev" => 2,
        "mars" => 3,
        "avril" | "avr" => 4,
        "mai" => 5,
        "juin" => 6,
        "juillet" | "juil" => 7,
        "aout" => 8,
        "septembre" | "sept" => 9,
        "octobre" | "oct" => 10,
        "novembre" | "nov" => 11,
        "decembre" | "dec" => 12,
        _ => return None,
    })
}

// Sans année : première occurrence >= aujourd'hui (année suivante si passée ; 29 fév -> bissextile).
fn resolve(d: u32, m: u32, y: Option<i32>, today: NaiveDate) -> Option<NaiveDate> {
    match y {
        Some(y) => NaiveDate::from_ymd_opt(y, m, d),
        None => (today.year()..today.year() + 9)
            .find_map(|y| NaiveDate::from_ymd_opt(y, m, d).filter(|x| *x >= today)),
    }
}

// « le 12 » : ce mois si pas passé, sinon le prochain mois qui possède ce jour.
fn day_only(d: u32, today: NaiveDate) -> Option<NaiveDate> {
    let first = today.with_day(1)?;
    (0..14).find_map(|k| {
        first
            .checked_add_months(Months::new(k))?
            .with_day(d)
            .filter(|x| *x >= today)
    })
}

// Dates numériques ("12/03", "12/03/2027") ou "12 mars [2027]" ; `solo` autorise le jour seul (après « le »).
fn date_core(t: &[Tok], i: usize, today: NaiveDate, solo: bool) -> Option<(usize, NaiveDate)> {
    let w = t.get(i)?.n.as_str();
    if w.contains('/') {
        let p: Vec<&str> = w.split('/').collect();
        if p.len() < 2 || p.len() > 3 {
            return None;
        }
        let y = match p.get(2) {
            None => None,
            Some(y) if y.len() == 2 => Some(2000 + num(y, 2)? as i32),
            Some(y) if y.len() == 4 => Some(num(y, 4)? as i32),
            _ => return None,
        };
        return resolve(num(p[0], 2)?, num(p[1], 2)?, y, today).map(|d| (1, d));
    }
    let d = num(w, 2)?;
    if let Some(m) = t.get(i + 1).and_then(|x| month(&x.n)) {
        let y = t
            .get(i + 2)
            .filter(|x| x.n.len() == 4)
            .and_then(|x| num(&x.n, 4))
            .filter(|y| (2000..3000).contains(y));
        return resolve(d, m, y.map(|y| y as i32), today).map(|x| (2 + y.is_some() as usize, x));
    }
    if solo { day_only(d, today).map(|x| (1, x)) } else { None }
}

// Essaie de reconnaître un groupe de mots à la position i : (nombre de mots, effet).
fn step(t: &[Tok], i: usize, now: NaiveDateTime) -> Option<(usize, Hit)> {
    let today = now.date();
    let w = t[i].n.as_str();
    let nx = |k: usize| t.get(i + k).map(|x| x.n.as_str());
    let add = |n: u64| today.checked_add_days(Days::new(n));
    let date = |n: usize, d: Option<NaiveDate>| d.map(|d| (n, Hit::Date(d)));

    if t[i].o == "!" {
        return Some((1, Hit::Important));
    }
    if let Some(l) = t[i].o.strip_prefix('#') {
        if !l.is_empty() && l.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            return Some((1, Hit::List(l.to_string())));
        }
    }
    // compteur : x8, 8x, 8 fois
    if let Some(n) = w.strip_prefix('x').and_then(cnt).or_else(|| w.strip_suffix('x').and_then(cnt)) {
        return Some((1, Hit::Count(n)));
    }
    if nx(1) == Some("fois") {
        if let Some(n) = cnt(w) {
            return Some((2, Hit::Count(n)));
        }
    }
    // heures
    if let Some(h) = hm(w) {
        return Some((1, Hit::Time(h, false)));
    }
    if w == "a" {
        if let Some(h) = nx(1).and_then(hm) {
            return Some((2, Hit::Time(h, false)));
        }
    }
    let tm = |h| NaiveTime::from_hms_opt(h, 0, 0).unwrap();
    match (w, nx(1)) {
        ("midi", _) => return Some((1, Hit::Time(tm(12), false))),
        ("matin", _) => return Some((1, Hit::Time(tm(9), false))),
        ("soir", _) => return Some((1, Hit::Time(tm(19), false))),
        ("ce", Some("matin")) => return Some((2, Hit::Time(tm(9), false))),
        ("ce", Some("soir")) => return Some((2, Hit::Time(tm(19), true))),
        _ => {}
    }
    // jours de la semaine : prochaine occurrence strictement après aujourd'hui
    let jours = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];
    let cur = today.weekday().num_days_from_monday() as u64;
    if let Some(k) = jours.iter().position(|j| *j == w) {
        let delta = (k as u64 + 7 - cur) % 7;
        return date(1 + (nx(1) == Some("prochain")) as usize, add(if delta == 0 { 7 } else { delta }));
    }
    match w {
        "aujourd'hui" | "aujourdhui" | "auj" => date(1, Some(today)),
        "demain" => date(1, add(1)),
        "apres-demain" => date(1, add(2)),
        "semaine" if nx(1) == Some("prochaine") => date(2, add(7 - cur)),
        "le" => date_core(t, i + 1, today, true).map(|(n, d)| (n + 1, Hit::Date(d))),
        "dans" => {
            let a = nx(1)?;
            let p = a.find(|c: char| !c.is_ascii_digit()).unwrap_or(a.len());
            let n = num(&a[..p], 4)?;
            let (unit, used) = if p < a.len() { (&a[p..], 2) } else { (nx(2)?, 3) };
            let rel = |d: chrono::Duration| {
                let r = now.checked_add_signed(d)?;
                Some(Hit::Rel(r.date().and_time(NaiveTime::from_hms_opt(r.hour(), r.minute(), 0)?)))
            };
            let hit = match unit {
                "jour" | "jours" | "j" => add(n as u64).map(Hit::Date),
                "semaine" | "semaines" | "sem" => add(n as u64 * 7).map(Hit::Date),
                "mois" => today.checked_add_months(Months::new(n)).map(Hit::Date),
                "heure" | "heures" | "h" => rel(chrono::Duration::hours(n as i64)),
                "minute" | "minutes" | "min" => rel(chrono::Duration::minutes(n as i64)),
                _ => None,
            };
            hit.map(|h| (used, h))
        }
        _ => date_core(t, i, today, false).map(|(n, d)| (n, Hit::Date(d))),
    }
}

pub fn parse(input: &str, now: NaiveDateTime) -> Parsed {
    let t: Vec<Tok> = input
        .split_whitespace()
        .map(|o| Tok { o, s: o.as_ptr() as usize - input.as_ptr() as usize, n: norm(o) })
        .collect();
    let mut p = Parsed::default();
    let (mut time, mut forced) = (None, false);
    let mut skip = vec![false; t.len()];
    let mut i = 0;
    while i < t.len() {
        let Some((n, hit)) = step(&t, i, now) else {
            i += 1;
            continue;
        };
        // le premier de chaque sorte gagne, mais tout est consommé (retiré du titre)
        let kind = match hit {
            Hit::Date(d) => {
                p.due.get_or_insert(d);
                Kind::Date
            }
            Hit::Time(h, f) => {
                if time.is_none() {
                    (time, forced) = (Some(h), f);
                }
                Kind::Time
            }
            Hit::Rel(r) => {
                if p.due.is_none() && time.is_none() {
                    (p.due, time) = (Some(r.date()), Some(r.time()));
                }
                Kind::Time
            }
            Hit::List(l) => {
                p.list.get_or_insert(l);
                Kind::List
            }
            Hit::Important => {
                p.important = true;
                Kind::Important
            }
            Hit::Count(c) => {
                p.target.get_or_insert(c);
                Kind::Count
            }
        };
        let last = &t[i + n - 1];
        p.spans.push((t[i].s..last.s + last.o.len(), kind));
        skip[i..i + n].fill(true);
        i += n;
    }
    // heure sans date : aujourd'hui si elle est après maintenant, sinon demain
    if let Some(h) = time {
        let d = *p.due.get_or_insert(if forced || h > now.time() {
            now.date()
        } else {
            now.date().succ_opt().unwrap_or(now.date())
        });
        p.remind = Some(d.and_time(h));
    }
    p.title = t.iter().zip(&skip).filter(|(_, s)| !**s).map(|(x, _)| x.o).collect::<Vec<_>>().join(" ");
    if p.title.is_empty() {
        p.title = input.trim().to_string();
    }
    p
}

/// Champs « échéance » / « rappel » : None si rien compris.
pub fn parse_when(input: &str, now: NaiveDateTime) -> Option<(NaiveDate, Option<NaiveTime>)> {
    let p = parse(input, now);
    p.due.map(|d| (d, p.remind.map(|r| r.time())))
}

/// "aujourd'hui", "demain", "hier", "lun. 30 nov.", "12 mars 2027", suivi de " 18:00" si heure.
pub fn describe(d: NaiveDate, t: Option<NaiveTime>, today: NaiveDate) -> String {
    const JOURS: [&str; 7] = ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."];
    const MOIS: [&str; 12] = [
        "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc.",
    ];
    let m = MOIS[d.month0() as usize];
    let mut s = match (d - today).num_days() {
        0 => "aujourd'hui".to_string(),
        1 => "demain".to_string(),
        -1 => "hier".to_string(),
        _ if d.year() != today.year() => format!("{} {} {}", d.day(), m, d.year()),
        _ => format!("{} {} {}", JOURS[d.weekday().num_days_from_monday() as usize], d.day(), m),
    };
    if let Some(t) = t {
        s += &t.format(" %H:%M").to_string();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(y: i32, m: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, mi, 0).unwrap()
    }
    fn da(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }
    // mercredi 25 novembre 2026, 10:00
    fn now() -> NaiveDateTime {
        dt(2026, 11, 25, 10, 0)
    }
    fn p(s: &str) -> Parsed {
        parse(s, now())
    }
    fn due(s: &str) -> Option<NaiveDate> {
        p(s).due
    }

    #[test]
    fn exemple_complet() {
        let r = p("beurre demain 18h #courses !");
        assert_eq!(r.title, "beurre");
        assert_eq!(r.due, Some(da(2026, 11, 26)));
        assert_eq!(r.remind, Some(dt(2026, 11, 26, 18, 0)));
        assert_eq!(r.list.as_deref(), Some("courses"));
        assert!(r.important);
    }

    #[test]
    fn spans_triees_et_exactes() {
        let r = p("beurre demain 18h");
        assert_eq!(r.spans, vec![(7..13, Kind::Date), (14..17, Kind::Time)]);
        let r = p("a #b ! x3");
        assert_eq!(r.spans.iter().map(|s| s.1).collect::<Vec<_>>(), [Kind::List, Kind::Important, Kind::Count]);
    }

    #[test]
    fn jours_de_semaine() {
        let r = p("appeler maman lundi");
        assert_eq!(r.title, "appeler maman");
        assert_eq!(r.due, Some(da(2026, 11, 30)));
        assert_eq!(r.remind, None);
        let r = p("voir Paul lundi prochain");
        assert_eq!((r.title.as_str(), r.due), ("voir Paul", Some(da(2026, 11, 30))));
        assert_eq!(due("x mercredi"), Some(da(2026, 12, 2))); // strictement après aujourd'hui
        assert_eq!(due("x dimanche"), Some(da(2026, 11, 29)));
    }

    #[test]
    fn relatifs() {
        assert_eq!(due("x Après-Demain"), Some(da(2026, 11, 27)));
        assert_eq!(due("x apres-demain"), Some(da(2026, 11, 27)));
        assert_eq!(due("x AUJOURD'HUI"), Some(da(2026, 11, 25)));
        assert_eq!(due("x auj"), Some(da(2026, 11, 25)));
        assert_eq!(due("x semaine prochaine"), Some(da(2026, 11, 30)));
        assert_eq!(due("payer dans 3 jours"), Some(da(2026, 11, 28)));
        assert_eq!(p("payer dans 3 jours").title, "payer");
        assert_eq!(due("x dans 2 semaines"), Some(da(2026, 12, 9)));
        assert_eq!(due("x dans 1 mois"), Some(da(2026, 12, 25)));
        assert_eq!(due("x dans 5j"), Some(da(2026, 11, 30)));
    }

    #[test]
    fn jour_du_mois() {
        let r = p("rapport le 12");
        assert_eq!((r.title.as_str(), r.due), ("rapport", Some(da(2026, 12, 12))));
        assert_eq!(due("x le 25"), Some(da(2026, 11, 25)));
        assert_eq!(due("x le 31"), Some(da(2026, 12, 31))); // novembre n'a que 30 jours
        assert_eq!(due("x le 28"), Some(da(2026, 11, 28)));
    }

    #[test]
    fn dates_numeriques_et_mois() {
        assert_eq!(due("x le 12/03"), Some(da(2027, 3, 12)));
        assert_eq!(due("x 12/03"), Some(da(2027, 3, 12)));
        assert_eq!(due("x 12/03/2027"), Some(da(2027, 3, 12)));
        assert_eq!(due("x 12/03/28"), Some(da(2028, 3, 12)));
        assert_eq!(due("x 12 mars"), Some(da(2027, 3, 12)));
        assert_eq!(due("x 12 mars 2028"), Some(da(2028, 3, 12)));
        assert_eq!(due("x 30 nov."), Some(da(2026, 11, 30)));
        assert_eq!(due("x 3 déc"), Some(da(2026, 12, 3)));
        assert_eq!(due("x 15 janv"), Some(da(2027, 1, 15)));
        assert_eq!(due("x 1 août"), Some(da(2027, 8, 1)));
        assert_eq!(due("x 29 fév"), Some(da(2028, 2, 29)));
        assert_eq!(due("x 25 nov"), Some(da(2026, 11, 25)));
        assert_eq!(due("x 24 nov"), Some(da(2027, 11, 24)));
    }

    #[test]
    fn heures() {
        let r = p("réunion 9h");
        assert_eq!((r.title.as_str(), r.remind), ("réunion", Some(dt(2026, 11, 26, 9, 0))));
        assert_eq!(r.due, Some(da(2026, 11, 26)));
        assert_eq!(p("x 18:30").remind, Some(dt(2026, 11, 25, 18, 30)));
        assert_eq!(p("x 10h").remind, Some(dt(2026, 11, 26, 10, 0))); // pas strictement après now
        let r = p("appel à 8h");
        assert_eq!((r.title.as_str(), r.remind), ("appel", Some(dt(2026, 11, 26, 8, 0))));
        assert_eq!(p("x a 14h30").remind, Some(dt(2026, 11, 25, 14, 30)));
        assert_eq!(p("x midi demain").remind, Some(dt(2026, 11, 26, 12, 0)));
        assert_eq!(p("x demain matin").remind, Some(dt(2026, 11, 26, 9, 0)));
        assert_eq!(p("x demain soir").remind, Some(dt(2026, 11, 26, 19, 0)));
    }

    #[test]
    fn date_et_heure() {
        let r = p("dentiste 3 déc à 14h30");
        assert_eq!(r.title, "dentiste");
        assert_eq!(r.remind, Some(dt(2026, 12, 3, 14, 30)));
        assert_eq!(r.due, Some(da(2026, 12, 3)));
    }

    #[test]
    fn dans_n_heures() {
        assert_eq!(parse_when("dans 2 h", now()), Some((da(2026, 11, 25), NaiveTime::from_hms_opt(12, 0, 0))));
        assert_eq!(parse_when("dans 2h", now()), Some((da(2026, 11, 25), NaiveTime::from_hms_opt(12, 0, 0))));
        assert_eq!(parse_when("dans 90 min", now()), Some((da(2026, 11, 25), NaiveTime::from_hms_opt(11, 30, 0))));
        assert_eq!(parse_when("dans 15 heures", now()), Some((da(2025 + 1, 11, 26), NaiveTime::from_hms_opt(1, 0, 0))));
        assert_eq!(p("sieste dans 2 heures").title, "sieste");
    }

    #[test]
    fn ce_soir_et_mars() {
        let r = p("voir Mars ce soir");
        assert_eq!(r.title, "voir Mars");
        assert_eq!(r.due, Some(da(2026, 11, 25)));
        assert_eq!(r.remind, Some(dt(2026, 11, 25, 19, 0)));
        let r = parse("x ce soir", dt(2026, 11, 25, 23, 0));
        assert_eq!(r.remind, Some(dt(2026, 11, 25, 19, 0))); // forcé aujourd'hui
        assert_eq!(due("mars"), None);
        assert_eq!(p("mars").title, "mars");
    }

    #[test]
    fn compteurs() {
        let r = p("sport x8");
        assert_eq!((r.title.as_str(), r.target), ("sport", Some(8)));
        let r = p("pompes 20 fois");
        assert_eq!((r.title.as_str(), r.target), ("pompes", Some(20)));
        assert_eq!(p("lire 8x").target, Some(8));
        assert_eq!(p("lire x999").target, Some(999));
        for s in ["a x0", "a x1", "a x1000", "a 99999999999 fois", "a 1 fois", "a x", "a 0x"] {
            assert_eq!(p(s).target, None, "{s}");
            assert_eq!(p(s).title, s, "{s}");
        }
    }

    #[test]
    fn important_et_liste() {
        let r = p("!");
        assert!(r.important);
        assert_eq!(r.title, "!");
        let r = p("acheter pain !");
        assert_eq!((r.title.as_str(), r.important), ("acheter pain", true));
        assert!(!p("super!").important);
        assert_eq!(p("a #été-2026_b").list.as_deref(), Some("été-2026_b"));
        assert_eq!(p("a #").list, None);
        assert_eq!(p("a #a#b").list, None);
    }

    #[test]
    fn jamais_consomme() {
        for s in ["le 99", "25h", "dans la rue", "dans 3", "dans 3 chats", "à la maison", "le chat", "42", "voir 12 chats", "a 24h"] {
            let r = p(s);
            assert_eq!(r, Parsed { title: s.to_string(), ..Default::default() }, "{s}");
        }
        assert_eq!(due("x 31/02"), None);
        assert_eq!(due("x 12/13"), None);
    }

    #[test]
    fn vide_et_robuste() {
        assert_eq!(p(""), Parsed::default());
        assert_eq!(p("   ").title, "");
        // titre vide -> saisie trimée telle quelle
        assert_eq!(p("  demain  ").title, "demain");
        assert_eq!(p("  demain  ").due, Some(da(2026, 11, 26)));
        for s in [
            "🎉", "🎉 fête demain 😀 18h", "éàü ñ 日本語 le 12 mars", "le", "dans", "à", "#", "x", "/", "//", "1/", "12/", "/12",
            "99999999999999999999 jours", "dans 99999999999 jours", "dans 9999 mois", "dans 9999 heures", "le 0", "0/0", "0:0",
            "h", ":", "18h:", "18:", "1h2", "ce", "semaine", "\u{0}", "a\u{200b}b", "İstanbul demain",
        ] {
            let r = p(s);
            for (rg, _) in &r.spans {
                assert!(s.is_char_boundary(rg.start) && s.is_char_boundary(rg.end), "{s}");
            }
            let _ = parse_when(s, now());
        }
        let s = "🎉 fête demain 😀";
        let r = p(s);
        assert_eq!(r.title, "🎉 fête 😀");
        assert_eq!(&s[r.spans[0].0.clone()], "demain");
    }

    #[test]
    fn parse_when_champs() {
        assert_eq!(parse_when("demain 18h", now()), Some((da(2026, 11, 26), NaiveTime::from_hms_opt(18, 0, 0))));
        assert_eq!(parse_when("lundi", now()), Some((da(2026, 11, 30), None)));
        assert_eq!(parse_when("12/03", now()), Some((da(2027, 3, 12), None)));
        assert_eq!(parse_when("blabla", now()), None);
        assert_eq!(parse_when("", now()), None);
    }

    #[test]
    fn premier_gagne() {
        let r = p("x demain lundi");
        assert_eq!(r.due, Some(da(2026, 11, 26)));
        assert_eq!(r.title, "x");
    }

    #[test]
    fn describe_formats() {
        let today = da(2026, 11, 25);
        assert_eq!(describe(today, None, today), "aujourd'hui");
        assert_eq!(describe(da(2026, 11, 26), None, today), "demain");
        assert_eq!(describe(da(2026, 11, 24), None, today), "hier");
        assert_eq!(describe(da(2026, 11, 30), None, today), "lun. 30 nov.");
        assert_eq!(describe(da(2026, 12, 3), None, today), "jeu. 3 déc.");
        assert_eq!(describe(da(2027, 3, 12), None, today), "12 mars 2027");
        assert_eq!(describe(da(2026, 11, 26), NaiveTime::from_hms_opt(18, 0, 0), today), "demain 18:00");
        assert_eq!(describe(today, NaiveTime::from_hms_opt(9, 5, 0), today), "aujourd'hui 09:05");
    }

    #[test]
    fn rapidite() {
        let n = now();
        let s = "appeler maman demain à 18h30 #courses ! pour la réunion x8 le 12 mars";
        let k = 20_000;
        let t0 = std::time::Instant::now();
        for _ in 0..k {
            std::hint::black_box(parse(std::hint::black_box(s), n));
        }
        let us = t0.elapsed().as_secs_f64() * 1e6 / k as f64;
        println!("parse : {us:.2} µs par appel");
        if !cfg!(debug_assertions) {
            assert!(us < 50.0, "trop lent : {us} µs");
        }
    }
}
