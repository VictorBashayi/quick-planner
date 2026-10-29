# Quick Planner (Rust) — design

Native rewrite of the Electron planner in `~/planner-app`. Goal: same workflow, far lower
RAM/CPU, a calmer and less generic look, one `.exe` with no Node/npx.

## Behaviour (carried over)
- Frameless window, hidden to tray on close. `Alt+P` toggles it; tray left-click toggles,
  tray menu has Open / Quit.
- Buckets: Today (`now`), This week (`soon`), Someday (`later`), Done (`done`).
- Quick-add box at the top; target bucket picked with the chips or `Ctrl+1/2/3`.
  `Esc` in the box hides the window.
- Each task: checkbox, title, optional description, move menu, delete.
  Click the title/description to edit; `Enter` commits, `Esc` reverts, empty title deletes.
- Checking a task sends it to Done and remembers its bucket; unchecking restores it.
- Buckets collapse; Done starts collapsed.
- Pin button toggles always-on-top (on by default).
- Drag a task by press-and-move: a ghost follows the pointer, the bucket under it
  highlights, and the drop inserts the task at the hovered position (reorder works too).

## Data
Same file and JSON shape as the Electron app, so tasks carry over and either app can read it:
`%APPDATA%\quick-planner\planner-tasks.json`
(`tasks[] {id,text,desc,bucket,completed,createdAt,completedAt?,previousBucket?}`,
`selectedBucket`, `collapsed`, `nextId`). Saves are debounced (300 ms) and atomic
(write temp file, rename).

## Tech
- Rust + Slint, software renderer, winit backend. No GPU context, no web engine.
- `tray-icon` + `global-hotkey` deliver events through callbacks that hop onto the Slint
  event loop — nothing polls, so idle CPU is ~0.
- Windows 11 rounded corners via DWM; single instance via a named mutex.
- Release build: LTO, 1 codegen unit, stripped, `windows_subsystem = "windows"`.

## Look
Calm, minimal, "coffee in a porcelain cup".
- Porcelain `#FAF9F7` background, espresso `#2E221B` text, coffee `#B0703A` accent,
  crema `#EADBC8` hairlines/selection, muted `#8C8079` secondary text.
- Bucket colour weakens with time distance: Today full coffee, This week medium,
  Someday pale latte, Done grey. That gradient is the one expressive element.
- Young Serif (OFL, embedded) for bucket names only; Segoe UI Variable for everything else.
- Sentence case, no emoji, no all-caps labels.
