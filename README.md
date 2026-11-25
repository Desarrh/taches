# Tâches

A fast, local to-do app for Windows, written in Rust (egui, CPU rendering). Inspired by Microsoft To Do, with no account and no sync: everything stays in `%APPDATA%\Taches\taches.json`. A single portable exe, no admin rights.

- **To Do basics**: My Day, Important, Planned, Tasks and your own lists (create, rename, reorder by drag or menu, pick an icon). Tasks have a title, note, due date, reminder, star and steps. Drag a task onto a list, My Day, Important or Planned; drag it onto another task to reorder.
- **Natural input** (French): `beurre demain 18h #courses !` gives the title, due date and reminder, list (created if needed) and importance, with a live preview while you type. Understands `demain`, `lundi`, `dans 3 jours`, `18h30`, `le 12`, `3 déc`, `ce soir`, `x8` / `8 fois`…
- **Ding** when a task is done, with a small burst animation. A finished task stays in place 5 s with an Undo button before moving down.
- **Milestones**: steps unlocked one at a time (only the next one shows, with a progress bar; the last one completes the task), and **counter tasks** ("do it 8 times"): each press plays a rising note, a distinct final chime at 8/8, optional reset every day or every week for habits.
- **Pomodoro tab** (Ctrl+0): big countdown inside a filling ring, phase and cycle (Work 2/4), Start/Pause, Skip, Reset. Works without a task (free sessions are counted apart) or attached to one: 25/5 by default, long break every 4 cycles, all adjustable. Time spent is saved on the task. The timer shows in the window title and the tray icon tooltip; soft chime at the end of each phase.
- **Smart My Day**: suggestions (overdue, due today, recently added, forgotten for a while) added with one key (1–9). My Day empties itself every night.
- **Reminders**: discreet Windows notification (no sound) at the set time; the app keeps running in the notification area and can start with Windows.
- **Keyboard**: Ctrl+N new task, Space check (counter +1), Shift+Space counter −1, ↑↓ move, Enter edit, Delete delete, Ctrl+Z undo, Ctrl+D My Day, Ctrl+I important, Ctrl+T due date, Ctrl+P pomodoro, Ctrl+J suggestions, Ctrl+F search (fuzzy, accent-insensitive), Ctrl+1…9 views and lists, Ctrl+K command palette, F1 help.

All sounds are synthesized in code (no files, no audio library). Volume and mute are in the settings.

`Taches.exe --ajouter "beurre demain #courses"` adds a task (forwarded to the running instance).

Build: `cargo build --release`. MIT license.
