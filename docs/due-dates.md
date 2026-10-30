# Due dates and reminders — spec

Status: draft for approval, 2026-10-30.

## Decisions (from the user)
- Entry: **both** typed shorthand in the add box and a picker on each row.
- A due date is a **date, with an optional time**.
- Reminders arrive as **Windows notifications** (toasts).
- A task **moves into Today on its own** when its due day arrives.

## Data
Two new optional fields on each task, so old files still load and the Electron shape is
only extended:

```json
{ "due": "2026-11-07", "reminded": false }
{ "due": "2026-11-07T15:00", "reminded": true }
```

- `due` is a local calendar date plus optional local time, kept as text. No time zones:
  "Friday 3pm" means 3pm wherever the PC is.
- `reminded` (default false) stops a reminder firing twice. Setting or changing `due`
  resets it.
- Due edits go through `App::changed()`, so undo/redo covers them for free.

## Date maths
No new date crate. Today's date comes from `GetLocalTime` (already used). Adding days and
finding weekdays use a small pure "days since 1970" conversion (Howard Hinnant's
`days_from_civil` and its inverse, ~20 lines). This lives in a new pure module,
`src/due.rs`, with the parser and labels, so all of it is unit tested.

## Entry 1: typed shorthand
Date words are read **only from the end of the title**, so "Plan friday party" stays as it
is, while "Essay fri" and "Call mum tomorrow at 3pm" get a due date and lose those words.

| Typed | Means |
|---|---|
| `today`, `tomorrow` / `tmrw` | that day |
| `mon` … `sun`, `monday` … `sunday` | the next such day; today's weekday means today |
| `7 nov`, `nov 7`, `7/11` | that date (day first); next year if it has passed |
| `3pm`, `3 pm`, `3:30pm`, `15:00`, optional `at` before it | the time; time alone means today |

- Short forms: `tdy`, `tmr`, `tmrw`, `tmw`, `tomo`, `2moro`, `weds`, and any prefix of 3+
  letters of a weekday or month.
- Typos: one in names of 5 to 7 letters, two in longer ones (`tommorow`, `wensday`,
  `novmber`). Words under 4 letters must match exactly, so "Call tom" stays as it is.
- While you type, the right side of the chip row previews the result and where it will go
  ("Due tomorrow, 3:00 pm → This week").
- Escape hatch: none needed. Ctrl+Z undoes the add, and the picker can clear the date.
- **The date picks the bucket**, whatever chip is selected: due today or overdue goes to
  Today, within 7 days goes to This week, and later than that goes to Someday.

## Entry 2: row picker
A calendar icon joins the row's hover actions (before "…"). It opens a popup in the same
style as the Move menu:
- Quick picks: Today, Tomorrow, Next Monday.
- A month grid (7 × 6, ‹ › to change month). The cells come from a pure Rust function, so
  Slint only draws them.
- A time box ("15:00" or "3pm", parsed by the same parser), empty for no time.
- "Clear date" when a date is set.

## Display
- The row's existing small grey line (used today for "Completed today") shows the due date
  for open tasks: "Due today, 3:00 pm", "Due tomorrow", "Due Fri 7 Nov".
- In the last hour before a timed task, it reads "Due soon, 3:00 pm" in coffee.
- Overdue tasks read "Overdue, Tue 28 Oct" in the danger colour.
- The 5-second tick redraws the list only when a task turns due soon or overdue, or the day
  changes, because a redraw ends any edit in progress.
- The widget shows the time (or "Overdue") to the right of a task's title.
- Order inside a bucket stays manual. Nothing is sorted by date.

## Reminders
- **When:** at the due time, or at 9:00 on the day if there is no time. This is a constant.
- **Clock:** the existing 5-second widget timer also checks reminders, so no new timer is
  added and idle CPU stays flat. Wall-clock checks also cope with sleep and hibernate.
- **Missed while the PC was off or the app closed:** on launch, any due, unreminded open
  tasks fire. If there is more than one, a single summary toast fires ("3 tasks are due")
  instead of a burst.
- **Toast:** the task title is the heading, with "Due 3:00 pm" (plus the note's first line)
  as the body, and one button, "Mark done". Clicking the toast opens the planner. Both are
  handled in-process through the toast's Activated event, which works because the app is
  always running in the tray.
- **Plumbing:** the `tauri-winrt-notification` crate. Unpackaged apps need an
  AppUserModelID: on startup the app writes
  `HKCU\Software\Classes\AppUserModelId\QuickPlanner` (DisplayName, IconUri pointing to an
  icon PNG saved next to the tasks file) and calls `SetCurrentProcessExplicitAppUserModelID`.
  No installer and no admin rights are needed. This goes in a new `src/notify.rs`.

## Auto-move as dates come closer
When the day changes (checked on the same tick and on launch), open tasks move to the top
of the bucket their date now belongs in, but only closer (Someday to This week to Today),
never further away. Because it runs only on the day change, dragging a task out of Today
keeps it out for the rest of the day.

## Tests
Pure and table-driven: parser (each row above plus non-matches like "Plan friday party"),
day maths across month, year and leap-year edges, month grid, labels, "is this reminder
due now", and auto-move on a `State`. The toast and the registry write are checked by hand.

## Order of work (each step is shippable and gets its own commit)
1. Data fields, `due.rs` (date maths, parser, labels), typed entry with preview, row and
   widget labels, auto-move.
2. Reminders and toasts.
3. Row picker.

## Future plans
Not in this round, but planned to follow once due dates have settled in:
- Repeating tasks (daily, weekdays, weekly, monthly), where ticking one off sets the next
  due date.
- Snooze from the notification (10 minutes, 1 hour, tomorrow).
- Several reminders per task, or a reminder some time before it is due.
- A setting for the 9:00 reminder time used by date-only tasks.
- Sorting a bucket by due date.
- Syncing with calendars.

## Risks
- `tauri-winrt-notification` pulls in the `windows` crate. Expect a longer first release
  build and a slightly bigger exe. RAM should rise only while a toast is shown. Measure
  after step 2.
- AppUserModelID registration through the registry is the documented route for
  unpackaged apps on Windows 10 and later, but it is verified only by hand on this PC.
- The parser only reads day-first dates (`7/11` = 7 November).
