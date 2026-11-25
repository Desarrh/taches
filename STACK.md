# Tâches — stack technique

Refaire Microsoft To Do en Rust, plus rapide, local, avec des QoL. Dépôt public : https://github.com/Desarrh/taches.

## 1. But et direction

- **Problème** : To Do est lent à ouvrir, lié à un compte Microsoft, et n'a ni habitudes, ni paliers, ni pomodoro.
- **Objectifs** : fenêtre utilisable en moins de 300 ms, frappe et recherche en moins d'une image, CPU ~0 % au repos ; tout au clavier ; données locales seulement.
- **Non-objectifs** : compte, synchro, import To Do, partage de listes, pièces jointes.

## 2. Stack

| Domaine | Choix |
|---|---|
| Langage | Rust, édition 2024 |
| Interface | egui 0.37 + winit 0.30, rendu CPU (`softbuffer` + `hdrshot-egui-backend`), comme Mémoire et l'Explorateur |
| Socle | `socle` v0.1.6 : `ui` (thème, icônes, palette Ctrl+K, aide F1, toasts, champ de recherche, modale), `fuzzy`, `keys`, `settings` (lecture tolérante), `paths::write_atomic`, `crash`, `instance`, `testmode` |
| Dates | `chrono` (heure locale, sérialisation ISO) |
| Sons | synthèse maison en WAV mémoire, `PlaySoundW` (`SND_MEMORY` + `SND_ASYNC`) |
| Windows | `windows-sys` 0.61 : zone de notification (`Shell_NotifyIconW`, notifications `NIF_INFO`), registre `Run`, `PlaySoundW` |

## 3. Architecture

| Fichier | Rôle |
|---|---|
| `src/main.rs` | boucle winit, rendu CPU, instance unique, zone de notification, réveils programmés (`about_to_wait` → `App::tick`) |
| `src/app.rs` | interface : barre latérale, liste, détail, suggestions, réglages, palette, clavier ; pomodoro ; enregistrement différé |
| `src/model.rs` | données et règles sans interface : listes, tâches, compteurs, remise à zéro, Ma journée, suggestions, rappels, ordre |
| `src/nlp.rs` | saisie naturelle en français (`parse`, `parse_when`, `describe`), sans regex |
| `src/sound.rs` | sons synthétisés (ding, palier montant, final, pomodoro) ; `MUTE` en mode test |
| `src/tray.rs` | icône, menu, info-bulle (mini-minuteur), notification discrète, démarrage avec Windows |

**Flux** : le dessin collecte des `Act`, appliquées après l'image. `tick()` tourne même fenêtre cachée : rappels, phases du pomodoro, passage à minuit (Ma journée et compteurs), enregistrement. La boucle dort jusqu'au prochain évènement utile (rappel, minuit, seconde du pomodoro, écriture), sans sondage.

**Données** : un seul fichier `%APPDATA%\Taches\taches.json` (`TACHES_DIR` pour les essais), écrit après 400 ms de calme (3 s au plus) sur un thread, en écriture atomique ; écriture immédiate à la fermeture. Fichier illisible → copié en `.bad`, on repart d'une liste vide.

## 4. Direction artistique

Thème du socle (crème / bleu nuit, bascule ☀/🌙), icônes vectorielles du socle (20 proposées pour les listes). Doré `#F2B84B` pour l'étoile et l'éclat de la coche. Lignes en cartes arrondies 6, barre latérale `side`, panneau de détail `surface`. Ni gras ni emoji (police légère). Curseur de saisie fixe : pas de clignotement, donc aucun réveil au repos.

## 5. Conventions

- Commentaires et textes en français ; erreurs d'écriture remontées en toast rouge.
- Ma journée = `my_day == aujourd'hui` : elle se vide d'elle-même à minuit.
- Compteur : `target > 0` ; `period` = début du jour ou de la semaine (lundi) ; au changement de période, `count = 0` et la tâche redevient à faire.
- Une tâche terminée reste à sa place 5 s (`UNDO_SECS`) avec « Annuler ».
- **Règle n° 0** : en mode test (`SOCLE_TEST=1` / `--test-hors-ecran`), fenêtre invisible, `sound::MUTE`, pas d'icône de notification donc aucune notification, pas d'écriture dans le registre. `TACHES_CAPTURE=x.png` capture la fenêtre invisible (PrintWindow) puis quitte ; `TACHES_ESSAI=detail|suggestions|planifie` prépare un état.
- Git : auteur `Desarrh`.

## 6. Fonctionnalités

Testé par `cargo test --release` (36 tests : saisie naturelle, sons, règles du modèle, pomodoro, annulation, rendu complet sans fenêtre de 300 tâches dans toutes les vues). Vérifié par capture invisible : Ma journée, détail avec étapes une à une et aperçu de saisie, suggestions, Planifié.

Mesures (i9-9900K, release) :
- exe 6,6 Mo ;
- lancement jusqu'à 8 images dessinées et capturées : 360 ms ;
- RAM privée au repos 18,5 Mo (working set 33 Mo) ;
- CPU au repos : 0 ms sur 10 s ;
- saisie naturelle : 2,7 µs par appel ; génération d'un son : 0,1 à 1,1 ms.

**Pomodoro (v0.2.0)** : onglet dédié (Ctrl+0) avec le temps restant en grand dans un anneau qui se remplit (accent pour le travail, vert doux pour les pauses), phase et rang (« Travail 2/4 »), Démarrer / Pause / Reprendre (Espace), Passer, Réinitialiser. Un seul minuteur pour l'onglet, la barre de titre, la barre latérale et l'info-bulle de l'icône. Sans tâche, c'est une session libre (`free_spent`, compté à part) ; le sélecteur, ou un glisser d'une tâche sur « Pomodoro », l'attache à une tâche (`spent`). Le travail entamé est compté quand on passe ou réinitialise. L'anneau se redessine à 30 i/s seulement quand le minuteur tourne et que l'onglet est affiché ; en pause, aucun réveil.

## 7. Intégrations

- `Taches.exe --ajouter "texte"` : ajoute une tâche (saisie naturelle), transmis à l'instance en cours. Marqueur `TachesCli` dans la ressource de version, pour le Lanceur.
- `--demarrage` : démarrage caché dans la zone de notification (valeur `Run` écrite par l'exe).

## 8. Limites connues

- Non vérifié à la main (pas d'entrées simulées en test) : glisser-déposer, sons réels, notification Windows, menu de l'icône, démarrage avec Windows.
- Heures locales naïves : un changement d'heure peut décaler un rappel d'une heure.
- Pas de tâches récurrentes (« chaque lundi ») hors compteurs ; tri manuel, ou par date dans Planifié.
- Suppression d'une liste définitive (avec confirmation) ; seule la dernière tâche supprimée est annulable.

## 9. Pistes QoL (à valider)

1. Tâches récurrentes (`chaque lundi`, `tous les 3 jours`) dans la saisie naturelle.
2. Statistiques d'habitudes (séries, calendrier) pour les compteurs, et pour Bilan.
3. Raccourci global pour ajouter une tâche de n'importe où.
4. Mise à jour automatique `socle::update`.
