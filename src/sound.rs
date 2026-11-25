//! Sons synthétisés (sinus + partiels), WAV généré en mémoire, lecture via PlaySoundW.
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sound {
    Done,           // tâche terminée : ding de clochette
    Step(u32, u32), // palier i sur n : note pentatonique montante, timbre marimba
    Final,          // dernier palier : arpège majeur + cloche tenue
    Pomodoro,       // fin de cycle : carillon grave à deux notes
}

/// Mis à true par l'app en mode test : `play` ne fait rien.
pub static MUTE: AtomicBool = AtomicBool::new(false);
/// Tampon de lecture : doit survivre à la lecture asynchrone.
static BUF: Mutex<Vec<u8>> = Mutex::new(Vec::new());

const SR: u32 = 22_050;
// partiels : (rapport de fréquence, amplitude, vitesse de décroissance relative)
const MARIMBA: &[(f32, f32, f32)] = &[(1.0, 1.0, 1.0), (4.0, 0.12, 3.0), (10.0, 0.03, 6.0)];
const CLOCHE: &[(f32, f32, f32)] = &[(1.0, 1.0, 1.0), (2.76, 0.35, 1.6), (5.4, 0.12, 2.5)];

/// Ajoute une note dans `buf` à l'instant `t0` (s) : attaque 3 ms, décroissance exp., fondu final 10 ms.
fn note(buf: &mut [f32], t0: f32, f: f32, dur: f32, amp: f32, decay: f32, parts: &[(f32, f32, f32)]) {
    let start = (t0 * SR as f32) as usize;
    let n = ((dur * SR as f32) as usize).min(buf.len().saturating_sub(start));
    for k in 0..n {
        let t = k as f32 / SR as f32;
        let (att, fade) = ((t / 0.003).min(1.0), ((dur - t) / 0.010).clamp(0.0, 1.0));
        let s: f32 = parts
            .iter()
            .map(|&(r, a, d)| a * (-decay * d * t).exp() * (std::f32::consts::TAU * f * r * t).sin())
            .sum();
        buf[start + k] += amp * att * fade * s;
    }
}

/// Fréquence (Hz) de la note de rang i (1-based) : pentatonique depuis C5, plafonnée à ~2 octaves.
fn step_freq(i: u32) -> f32 {
    let k = i.saturating_sub(1).min(10);
    let semi = 12 * (k / 5) + [0, 2, 4, 7, 9][(k % 5) as usize];
    523.25 * 2f32.powf(semi as f32 / 12.0)
}

/// Fichier WAV complet (PCM 16 bits mono 22 050 Hz), volume 0..=1.
pub fn wav(s: Sound, volume: f32) -> Vec<u8> {
    let (dur, gain) = match s {
        Sound::Done => (0.35, 1.0),
        Sound::Step(..) => (0.18, 0.8),
        Sound::Final => (0.7, 1.0),
        Sound::Pomodoro => (0.5, 0.6),
    };
    let mut b = vec![0f32; (dur * SR as f32).round() as usize];
    match s {
        Sound::Done => note(&mut b, 0.0, 1318.5, dur, 1.0, 9.0, CLOCHE),
        Sound::Step(i, _) => note(&mut b, 0.0, step_freq(i), dur, 1.0, 16.0, MARIMBA),
        Sound::Final => {
            for (k, f) in [523.25, 659.25, 783.99, 1046.5].into_iter().enumerate() {
                note(&mut b, k as f32 * 0.08, f, 0.3, 0.6, 12.0, MARIMBA);
            }
            note(&mut b, 0.24, 1046.5, 0.46, 0.8, 4.0, CLOCHE);
        }
        Sound::Pomodoro => {
            note(&mut b, 0.0, 440.0, 0.3, 1.0, 7.0, CLOCHE);
            note(&mut b, 0.22, 329.63, 0.28, 1.0, 7.0, CLOCHE);
        }
    }
    // normalise la crête à volume × gain, puis quantifie en i16
    let peak = b.iter().fold(0f32, |m, x| m.max(x.abs())).max(1e-9);
    let k = volume.clamp(0.0, 1.0) * gain * 32767.0 / peak;
    let len = (b.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + len as usize);
    for part in [
        &b"RIFF"[..],
        &(36 + len).to_le_bytes(),
        b"WAVEfmt ",
        &16u32.to_le_bytes(),
        &1u16.to_le_bytes(), // PCM
        &1u16.to_le_bytes(), // mono
        &SR.to_le_bytes(),
        &(SR * 2).to_le_bytes(),
        &2u16.to_le_bytes(),
        &16u16.to_le_bytes(),
        b"data",
        &len.to_le_bytes(),
    ] {
        w.extend_from_slice(part);
    }
    for x in b {
        w.extend_from_slice(&((x * k).round() as i16).to_le_bytes());
    }
    w
}

/// Joue le son sans bloquer. Sans effet si muet, volume nul ou sous test.
pub fn play(s: Sound, volume: f32) {
    if MUTE.load(Ordering::Relaxed) || !(volume > 0.0) || cfg!(test) {
        return;
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
        let new = wav(s, volume);
        // verrou empoisonné toléré ; tenu pendant les appels pour sérialiser les appuis rapides
        let mut g = BUF.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0); // arrêt synchrone avant de libérer l'ancien tampon
            *g = new;
            PlaySoundW(g.as_ptr() as *const u16, std::ptr::null_mut(), SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(w: &[u8]) -> Vec<i16> {
        w[44..].chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect()
    }
    fn u32_at(w: &[u8], o: usize) -> u32 {
        u32::from_le_bytes(w[o..o + 4].try_into().unwrap())
    }
    fn peak(w: &[u8]) -> i32 {
        samples(w).iter().map(|&x| (x as i32).abs()).max().unwrap()
    }
    /// Fréquence estimée par passages à zéro entre 10 et 90 ms.
    fn freq(w: &[u8]) -> f32 {
        let s = samples(w);
        let (a, b) = (SR as usize / 100, SR as usize * 9 / 100);
        let z = s[a..b].windows(2).filter(|p| (p[0] < 0) != (p[1] < 0)).count();
        z as f32 / 2.0 / ((b - a) as f32 / SR as f32)
    }
    const TOUS: [(Sound, f32); 4] = [
        (Sound::Done, 0.35),
        (Sound::Step(1, 5), 0.18),
        (Sound::Final, 0.7),
        (Sound::Pomodoro, 0.5),
    ];

    #[test]
    fn entete_valide() {
        for (s, _) in TOUS {
            let w = wav(s, 0.5);
            assert_eq!(&w[0..4], b"RIFF");
            assert_eq!(&w[8..16], b"WAVEfmt ");
            assert_eq!(&w[36..40], b"data");
            assert_eq!(u32_at(&w, 4) as usize, w.len() - 8);
            assert_eq!(u32_at(&w, 40) as usize, w.len() - 44);
            assert_eq!(u32_at(&w, 24), SR);
            assert_eq!(u16::from_le_bytes([w[22], w[23]]), 1); // mono
            assert_eq!(u16::from_le_bytes([w[34], w[35]]), 16);
        }
    }

    #[test]
    fn durees() {
        for (s, d) in TOUS {
            let got = samples(&wav(s, 1.0)).len() as f32 / SR as f32;
            assert!((got - d).abs() < 0.002, "{s:?}: {got}");
        }
    }

    #[test]
    fn crete_et_volume() {
        for (s, _) in TOUS {
            for v in [0.2, 0.5, 1.0] {
                let p = peak(&wav(s, v)) as f32;
                assert!(p <= v * 32767.0 + 2.0, "{s:?} {v}: {p}");
                assert!(p >= v * 32767.0 * 0.5, "{s:?} {v}: trop faible {p}");
            }
        }
        // Done au volume demandé, Pomodoro plus discret
        assert!(peak(&wav(Sound::Done, 1.0)) > 32000);
        assert!(peak(&wav(Sound::Pomodoro, 1.0)) < peak(&wav(Sound::Done, 1.0)));
    }

    #[test]
    fn volume_zero_silence() {
        for (s, _) in TOUS {
            assert_eq!(peak(&wav(s, 0.0)), 0);
        }
    }

    #[test]
    fn step_monte() {
        let f: Vec<f32> = (1..=11).map(|i| freq(&wav(Sound::Step(i, 11), 1.0))).collect();
        assert!(f.windows(2).all(|p| p[1] > p[0] * 1.05), "{f:?}");
        assert!((f[0] - 523.25).abs() < 25.0, "{f:?}");
        // plafonné au-delà de la gamme
        assert_eq!(wav(Sound::Step(11, 40), 1.0), wav(Sound::Step(40, 40), 1.0));
    }

    #[test]
    fn sons_distincts() {
        assert_ne!(wav(Sound::Final, 1.0), wav(Sound::Done, 1.0));
        assert_ne!(wav(Sound::Pomodoro, 1.0), wav(Sound::Done, 1.0));
    }

    #[test]
    fn play_sous_test_ne_fait_rien() {
        play(Sound::Done, 1.0);
        assert!(BUF.lock().unwrap().is_empty()); // rien n'a été chargé ni joué
    }

    #[test]
    fn temps_de_generation() {
        for (s, _) in TOUS {
            let t = std::time::Instant::now();
            for _ in 0..100 {
                std::hint::black_box(wav(s, 1.0));
            }
            eprintln!("{s:?} : {:?} par son", t.elapsed() / 100);
        }
    }
}
