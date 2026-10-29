# Quick Planner

A small, calm task planner for Windows. It is one native `.exe` (Rust + Slint, software
rendering), so it needs no browser engine or Node runtime. It idles at about 0% CPU and
uses roughly 5 MB of private memory.

## Features

- **Buckets:** Today, This week, Someday and Done.
- **Quick add:** type in the box at the top. Pick the bucket with the chips or `Ctrl+1/2/3`.
- **Editing:** click a title or note to edit it. `Enter` saves, `Esc` reverts, and an empty title deletes the task.
- **Drag and drop:** press, hold briefly, then drag a task to reorder it or move it to another bucket.
- **Completing:** checking a task sends it to Done. Unchecking it puts it back where it was.
- **Clear completed:** "Clear" in the Done header deletes every finished task. Click it twice to confirm.
- **Undo and redo:** with the add box empty, `Ctrl+Z` undoes the last change and `Ctrl+Y` or `Ctrl+Shift+Z` redoes it. After a delete or clear, the footer offers Undo for a few seconds. The last 50 steps are kept until you quit.
- **Tray app:** closing the window hides it to the tray. `Alt+P` or a tray click toggles it.
- **Pin:** the pin button keeps the window on top.

### Desktop widget

A transparent card on the desktop shows today's open tasks. Tick one to complete it.

- Win+D (Show desktop) leaves it in place.
- The text switches between light and dark to stay readable on your wallpaper.
- Drag the header to move it, use the corner grip to resize it, and double-click the header to open the planner.
- It remembers its position and size.
- "Show or hide widget" in the tray menu toggles it.

## Data

Tasks are saved to `%APPDATA%\quick-planner\planner-tasks.json`. Writes are debounced and
atomic, and an unreadable file is backed up rather than overwritten.

## Building

You need Rust with the GNU toolchain (`x86_64-pc-windows-gnu`) and
[WinLibs MinGW-w64](https://winlibs.com/) on `PATH`. The toolchain's bundled `dlltool` can't
find `as` without it.

```sh
cargo test
cargo run                # debug build
cargo build --release    # target/release/quick-planner.exe
```

Only one copy runs at a time, so quit the installed app before running a dev build.

See [docs/design.md](docs/design.md) for behaviour, data format and visual design notes.

## Credits

Young Serif font by [The Young Serif Project Authors](https://github.com/noirblancrouge/YoungSerif), under the SIL Open Font License (`ui/fonts/OFL.txt`).

Built with [Slint](https://slint.dev), used under the Slint Royalty-free Desktop, Mobile, and Web Applications License.

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)

## Licence

Quick Planner's own code is under the [MIT License](LICENSE). The font and Slint keep their own licences, as noted above.
