//! Task state, the operations on it, and JSON persistence.
//!
//! The file format matches the old Electron app, so existing tasks load as-is.

use crate::due::{Date, Due};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Ordered nearest first, so `Now < Soon < Later`.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Bucket {
    #[default]
    Now,
    Soon,
    Later,
    Done,
}

impl Bucket {
    pub const ALL: [Bucket; 4] = [Bucket::Now, Bucket::Soon, Bucket::Later, Bucket::Done];

    pub fn key(self) -> &'static str {
        match self {
            Bucket::Now => "now",
            Bucket::Soon => "soon",
            Bucket::Later => "later",
            Bucket::Done => "done",
        }
    }

    pub fn from_key(key: &str) -> Option<Bucket> {
        Bucket::ALL.into_iter().find(|b| b.key() == key)
    }

    /// Where an open task due on `date` belongs: Today when due or overdue, This week
    /// within seven days, Someday after that.
    pub fn for_due(date: Date, today: Date) -> Bucket {
        match date.days() - today.days() {
            ..=0 => Bucket::Now,
            1..=7 => Bucket::Soon,
            _ => Bucket::Later,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: u64,
    pub text: String,
    #[serde(default)]
    pub desc: String,
    pub bucket: Bucket,
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_bucket: Option<Bucket>,
    /// Local "2026-11-07" or "2026-11-07T15:00"; see `due::Due`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    /// The reminder for `due` has been shown.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reminded: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct State {
    pub tasks: Vec<Task>,
    pub selected_bucket: Bucket,
    pub collapsed: BTreeMap<String, bool>,
    pub next_id: u64,
    /// Desktop widget placement in physical pixels: x, y, width, height.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub widget: Option<[i32; 4]>,
}

impl Default for State {
    fn default() -> Self {
        State {
            tasks: Vec::new(),
            selected_bucket: Bucket::Now,
            collapsed: BTreeMap::from([("done".to_string(), true)]),
            next_id: 1,
            widget: None,
        }
    }
}

impl Task {
    /// Case-insensitive substring match on title or note; an empty query matches all.
    pub fn matches(&self, query: &str) -> bool {
        let q = query.to_lowercase();
        self.text.to_lowercase().contains(&q) || self.desc.to_lowercase().contains(&q)
    }

    pub fn due(&self) -> Option<Due> {
        self.due.as_deref().and_then(Due::parse)
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

impl State {
    pub fn tasks_in(&self, bucket: Bucket) -> impl Iterator<Item = &Task> {
        self.tasks.iter().filter(move |t| t.bucket == bucket)
    }

    pub fn is_collapsed(&self, bucket: Bucket) -> bool {
        self.collapsed.get(bucket.key()).copied().unwrap_or(false)
    }

    pub fn toggle_collapsed(&mut self, bucket: Bucket) {
        let c = !self.is_collapsed(bucket);
        self.collapsed.insert(bucket.key().to_string(), c);
    }

    pub fn expand(&mut self, bucket: Bucket) {
        self.collapsed.insert(bucket.key().to_string(), false);
    }

    fn find_mut(&mut self, id: u64) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    /// Adds a task at the top of `bucket`. Returns its id, or `None` for blank text.
    pub fn add(&mut self, text: &str, bucket: Bucket) -> Option<u64> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        // Guard against a stale nextId in a hand-edited file.
        let id = self.next_id.max(self.tasks.iter().map(|t| t.id + 1).max().unwrap_or(1));
        self.next_id = id + 1;
        self.tasks.insert(
            0,
            Task {
                id,
                text: text.to_string(),
                desc: String::new(),
                bucket,
                completed: false,
                created_at: now_ms(),
                completed_at: None,
                previous_bucket: None,
                due: None,
                reminded: false,
            },
        );
        self.move_to(id, bucket, 0);
        if bucket != Bucket::Done {
            self.collapsed.insert(bucket.key().to_string(), false);
        }
        Some(id)
    }

    pub fn delete(&mut self, id: u64) {
        self.tasks.retain(|t| t.id != id);
    }

    /// Deletes every task in Done and returns how many went.
    pub fn clear_done(&mut self) -> usize {
        let before = self.tasks.len();
        self.tasks.retain(|t| t.bucket != Bucket::Done);
        before - self.tasks.len()
    }

    /// Moves a task into `bucket` so it ends up at position `index` among that
    /// bucket's tasks. `index` counts the task itself if it is already there, which
    /// is what the UI sees while dragging. Indexes past the end append.
    pub fn move_to(&mut self, id: u64, bucket: Bucket, index: usize) {
        let Some(pos) = self.tasks.iter().position(|t| t.id == id) else { return };
        let mut index = index;
        if self.tasks[pos].bucket == bucket {
            let from = self.tasks[..pos].iter().filter(|t| t.bucket == bucket).count();
            if from < index {
                index -= 1;
            }
        }
        let mut task = self.tasks.remove(pos);

        if bucket == Bucket::Done {
            if task.bucket != Bucket::Done {
                task.previous_bucket = Some(task.bucket);
                task.completed_at = Some(now_ms());
            }
            task.completed = true;
        } else {
            task.completed = false;
            task.completed_at = None;
            task.previous_bucket = None;
        }
        task.bucket = bucket;

        let slots: Vec<usize> = self
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.bucket == bucket)
            .map(|(i, _)| i)
            .collect();
        let at = match (slots.get(index), slots.last()) {
            (Some(&i), _) => i,
            (None, Some(&last)) => last + 1,
            (None, None) => self.tasks.len(),
        };
        self.tasks.insert(at, task);
    }

    /// Checking a task sends it to the top of Done; unchecking puts it back on top
    /// of the bucket it came from.
    pub fn toggle(&mut self, id: u64) {
        let Some(task) = self.tasks.iter().find(|t| t.id == id) else { return };
        if task.completed {
            let back = task.previous_bucket.filter(|b| *b != Bucket::Done).unwrap_or(Bucket::Now);
            self.move_to(id, back, 0);
        } else {
            self.move_to(id, Bucket::Done, 0);
        }
    }

    pub fn set_text(&mut self, id: u64, text: &str) {
        if let Some(t) = self.find_mut(id) {
            t.text = text.to_string();
        }
    }

    pub fn set_desc(&mut self, id: u64, desc: &str) {
        if let Some(t) = self.find_mut(id) {
            t.desc = desc.to_string();
        }
    }

    /// Sets or clears a task's due date. An open task moves to the top of the bucket its
    /// date belongs in (`Bucket::for_due`).
    pub fn set_due(&mut self, id: u64, due: Option<Due>, today: Date) {
        let Some(t) = self.find_mut(id) else { return };
        t.due = due.map(|d| d.stored());
        t.reminded = false;
        if let Some(d) = due {
            let b = Bucket::for_due(d.date, today);
            if t.bucket != Bucket::Done && t.bucket != b {
                self.move_to(id, b, 0);
            }
        }
    }

    /// For a new day: moves open tasks whose date has come closer up a bucket (Someday to
    /// This week to Today), to the top, keeping their order. Never moves a task further
    /// away, so one dragged out of Today stays out. Returns whether any moved.
    pub fn roll_over(&mut self, today: Date) -> bool {
        let moves: Vec<(u64, Bucket)> = self
            .tasks
            .iter()
            .filter(|t| t.bucket != Bucket::Done)
            .filter_map(|t| {
                let b = Bucket::for_due(t.due()?.date, today);
                (b < t.bucket).then_some((t.id, b))
            })
            .collect();
        for &(id, b) in moves.iter().rev() {
            self.move_to(id, b, 0);
        }
        !moves.is_empty()
    }

    /// Marks open tasks whose reminder is due as reminded and returns them.
    pub fn take_reminders(&mut self, today: Date, minute: u32) -> Vec<Task> {
        self.tasks
            .iter_mut()
            .filter(|t| !t.completed && !t.reminded && t.due().is_some_and(|d| d.remind_now(today, minute)))
            .map(|t| {
                t.reminded = true;
                t.clone()
            })
            .collect()
    }

    /// Puts back a task list from undo history. Reminders already shown stay shown while
    /// the date is unchanged, so undoing an older edit doesn't fire them again.
    pub fn restore(&mut self, mut tasks: Vec<Task>) {
        for t in &mut tasks {
            t.reminded |= self.tasks.iter().any(|o| o.id == t.id && o.due == t.due && o.reminded);
        }
        self.tasks = tasks;
    }

    /// Marks an open task done; a done one stays done.
    pub fn complete(&mut self, id: u64) {
        if self.tasks.iter().any(|t| t.id == id && !t.completed) {
            self.toggle(id);
        }
    }

    /// Trims edited text and drops tasks whose title was cleared.
    pub fn tidy(&mut self) {
        for t in &mut self.tasks {
            let text = t.text.trim();
            if text.len() != t.text.len() {
                t.text = text.to_string();
            }
            let desc = t.desc.trim();
            if desc.len() != t.desc.len() {
                t.desc = desc.to_string();
            }
        }
        self.tasks.retain(|t| !t.text.is_empty());
    }

    pub fn counts(&self) -> (usize, usize) {
        let done = self.tasks.iter().filter(|t| t.completed).count();
        (self.tasks.len() - done, done)
    }
}

/// Undo and redo for the task list, kept as whole-list snapshots. Only the tasks
/// are tracked, so collapsing a section or switching buckets is not a step.
pub struct History {
    last: Vec<Task>,
    undo: Vec<Vec<Task>>,
    redo: Vec<Vec<Task>>,
}

impl History {
    const LIMIT: usize = 50;

    pub fn new(tasks: &[Task]) -> Self {
        History { last: tasks.to_vec(), undo: Vec::new(), redo: Vec::new() }
    }

    /// Notes the list after a change. Returns false when nothing actually changed.
    pub fn record(&mut self, tasks: &[Task]) -> bool {
        if tasks == self.last.as_slice() {
            return false;
        }
        let before = std::mem::replace(&mut self.last, tasks.to_vec());
        self.undo.push(before);
        if self.undo.len() > Self::LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        true
    }

    /// The list as it was before the latest change, if there is one.
    pub fn undo(&mut self) -> Option<Vec<Task>> {
        let tasks = self.undo.pop()?;
        self.redo.push(std::mem::replace(&mut self.last, tasks.clone()));
        Some(tasks)
    }

    pub fn redo(&mut self) -> Option<Vec<Task>> {
        let tasks = self.redo.pop()?;
        self.undo.push(std::mem::replace(&mut self.last, tasks.clone()));
        Some(tasks)
    }
}

pub fn data_path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("quick-planner").join("planner-tasks.json")
}

/// Loads the state. A file that exists but can't be parsed is copied aside
/// first, so starting fresh never overwrites the user's tasks.
pub fn load(path: &Path) -> State {
    let Ok(raw) = std::fs::read_to_string(path) else { return State::default() };
    match serde_json::from_str::<State>(&raw) {
        Ok(state) => state,
        Err(_) => {
            let backup = path.with_extension(format!("json.unreadable-{}", now_ms()));
            let _ = std::fs::copy(path, backup);
            State::default()
        }
    }
}

/// Writes to a temp file and renames it over the old one, so a crash mid-save
/// can't leave a half-written file.
pub fn save(path: &Path, state: &State) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(state)?)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(s: &State, b: Bucket) -> Vec<u64> {
        s.tasks_in(b).map(|t| t.id).collect()
    }

    fn state_with(n: usize, b: Bucket) -> State {
        let mut s = State::default();
        for i in 0..n {
            s.add(&format!("task {i}"), b);
        }
        s
    }

    #[test]
    fn add_puts_new_tasks_on_top_and_ignores_blank() {
        let mut s = State::default();
        assert_eq!(s.add("   ", Bucket::Now), None);
        s.add("a", Bucket::Now);
        s.add("b", Bucket::Now);
        assert_eq!(ids(&s, Bucket::Now), vec![2, 1]);
        assert_eq!(s.tasks[0].text, "b");
    }

    #[test]
    fn add_uncollapses_target_bucket() {
        let mut s = State::default();
        s.toggle_collapsed(Bucket::Soon);
        s.add("x", Bucket::Soon);
        assert!(!s.is_collapsed(Bucket::Soon));
    }

    #[test]
    fn expand_opens_a_collapsed_bucket_and_keeps_an_open_one_open() {
        let mut s = State::default();
        assert!(s.is_collapsed(Bucket::Done));
        s.expand(Bucket::Done);
        assert!(!s.is_collapsed(Bucket::Done));
        s.expand(Bucket::Done);
        assert!(!s.is_collapsed(Bucket::Done));
    }

    #[test]
    fn add_skips_ids_already_in_use() {
        let mut s = State::default();
        s.add("a", Bucket::Now);
        s.next_id = 1; // stale counter
        let id = s.add("b", Bucket::Now).unwrap();
        assert_eq!(id, 2);
    }

    #[test]
    fn clear_done_removes_only_finished_tasks() {
        let mut s = state_with(3, Bucket::Now); // now: 3,2,1
        s.toggle(1);
        s.toggle(3);
        assert_eq!(s.clear_done(), 2);
        assert_eq!(ids(&s, Bucket::Now), vec![2]);
        assert!(ids(&s, Bucket::Done).is_empty());
        assert_eq!(s.clear_done(), 0);
    }

    #[test]
    fn history_undoes_and_redoes_changes() {
        let mut s = state_with(1, Bucket::Now);
        let mut h = History::new(&s.tasks);
        assert!(!h.record(&s.tasks), "no change, no step");
        s.add("b", Bucket::Now);
        assert!(h.record(&s.tasks));
        s.delete(1);
        h.record(&s.tasks);

        s.tasks = h.undo().unwrap(); // b is back with 1
        assert_eq!(ids(&s, Bucket::Now), vec![2, 1]);
        s.tasks = h.undo().unwrap();
        assert_eq!(ids(&s, Bucket::Now), vec![1]);
        assert!(h.undo().is_none());

        s.tasks = h.redo().unwrap();
        assert_eq!(ids(&s, Bucket::Now), vec![2, 1]);
        // A new change drops what was left to redo.
        s.toggle(2);
        h.record(&s.tasks);
        assert!(h.redo().is_none());
        s.tasks = h.undo().unwrap();
        assert_eq!(ids(&s, Bucket::Now), vec![2, 1]);
    }

    #[test]
    fn history_keeps_the_last_fifty_steps() {
        let mut s = State::default();
        let mut h = History::new(&s.tasks);
        for i in 0..60 {
            s.add(&format!("t{i}"), Bucket::Now);
            h.record(&s.tasks);
        }
        let mut steps = 0;
        while let Some(tasks) = h.undo() {
            s.tasks = tasks;
            steps += 1;
        }
        assert_eq!(steps, 50);
        assert_eq!(s.tasks.len(), 10);
    }

    #[test]
    fn search_matches_title_or_note_ignoring_case() {
        let mut s = State::default();
        let id = s.add("Buy Milk", Bucket::Now).unwrap();
        s.set_desc(id, "from the Corner shop");
        let t = &s.tasks[0];
        assert!(t.matches("") && t.matches("milk") && t.matches("CORNER"));
        assert!(!t.matches("bread"));
    }

    #[test]
    fn due_date_picks_the_bucket() {
        let today = Date::new(2026, 10, 30).unwrap();
        let on = |m, d| Some(Due { date: Date::new(2026, m, d).unwrap(), time: None });
        let mut s = state_with(4, Bucket::Now); // now: 4,3,2,1
        s.set_due(1, on(11, 6), today); // 7 days
        s.set_due(2, on(11, 7), today); // 8 days
        s.set_due(3, on(10, 29), today); // overdue
        s.set_due(4, Some(Due { date: today, time: Some(900) }), today);
        assert_eq!(ids(&s, Bucket::Now), vec![4, 3]);
        assert_eq!(ids(&s, Bucket::Soon), vec![1]);
        assert_eq!(ids(&s, Bucket::Later), vec![2]);
        assert_eq!(s.tasks.iter().find(|t| t.id == 2).unwrap().due.as_deref(), Some("2026-11-07"));
    }

    #[test]
    fn roll_over_moves_tasks_closer_as_their_day_nears() {
        let day = |m, d| Date::new(2026, m, d).unwrap();
        let mut s = state_with(3, Bucket::Later); // later: 3,2,1
        for id in [3, 1] {
            s.set_due(id, Some(Due { date: day(11, 20), time: None }), day(10, 1));
        }
        s.add("today", Bucket::Now); // 4
        assert!(!s.roll_over(day(11, 1)));
        assert!(s.roll_over(day(11, 13)));
        assert_eq!(ids(&s, Bucket::Soon), vec![3, 1]);
        assert_eq!(ids(&s, Bucket::Later), vec![2]);
        assert!(s.roll_over(day(11, 20)));
        assert_eq!(ids(&s, Bucket::Now), vec![3, 1, 4]);
    }

    #[test]
    fn reminders_fire_once_and_survive_undo() {
        let today = Date::new(2026, 10, 30).unwrap();
        let mut s = state_with(3, Bucket::Now);
        let at = |time| Some(Due { date: today, time: Some(time) });
        s.set_due(1, at(600), today);
        s.set_due(2, at(700), today);
        s.set_due(3, at(600), today);
        s.toggle(3);
        let before = s.tasks.clone();
        let ids = |v: Vec<Task>| v.iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(ids(s.take_reminders(today, 650)), vec![1], "done tasks don't remind");
        assert_eq!(ids(s.take_reminders(today, 650)), Vec::<u64>::new());
        s.restore(before);
        assert!(s.take_reminders(today, 650).is_empty(), "undo keeps it reminded");
        s.set_due(1, at(800), today);
        assert!(!s.tasks.iter().find(|t| t.id == 1).unwrap().reminded, "a new date reminds again");
        s.complete(3);
        assert!(s.tasks.iter().find(|t| t.id == 3).unwrap().completed, "complete keeps done tasks done");
        s.complete(2);
        assert!(s.tasks.iter().find(|t| t.id == 2).unwrap().completed);
    }

    #[test]
    fn move_between_buckets_at_index() {
        let mut s = state_with(3, Bucket::Now); // now: 3,2,1
        s.add("s1", Bucket::Soon);
        s.add("s2", Bucket::Soon); // soon: 5,4
        s.move_to(2, Bucket::Soon, 1);
        assert_eq!(ids(&s, Bucket::Now), vec![3, 1]);
        assert_eq!(ids(&s, Bucket::Soon), vec![5, 2, 4]);
        s.move_to(3, Bucket::Soon, 99);
        assert_eq!(ids(&s, Bucket::Soon), vec![5, 2, 4, 3]);
    }

    #[test]
    fn reorder_within_bucket_counts_the_dragged_task() {
        let mut s = state_with(4, Bucket::Now); // 4,3,2,1
        // Dragging the first task to just below the third row (index 3 while it is still listed).
        s.move_to(4, Bucket::Now, 3);
        assert_eq!(ids(&s, Bucket::Now), vec![3, 2, 4, 1]);
        // Dropping onto its own slot is a no-op.
        s.move_to(2, Bucket::Now, 1);
        assert_eq!(ids(&s, Bucket::Now), vec![3, 2, 4, 1]);
        s.move_to(2, Bucket::Now, 2);
        assert_eq!(ids(&s, Bucket::Now), vec![3, 2, 4, 1]);
        s.move_to(1, Bucket::Now, 0);
        assert_eq!(ids(&s, Bucket::Now), vec![1, 3, 2, 4]);
    }

    #[test]
    fn move_into_empty_bucket() {
        let mut s = state_with(1, Bucket::Now);
        s.move_to(1, Bucket::Later, 0);
        assert_eq!(ids(&s, Bucket::Later), vec![1]);
        assert!(ids(&s, Bucket::Now).is_empty());
    }

    #[test]
    fn toggle_round_trips_to_original_bucket() {
        let mut s = state_with(2, Bucket::Soon);
        s.toggle(1);
        let t = s.tasks.iter().find(|t| t.id == 1).unwrap();
        assert_eq!(t.bucket, Bucket::Done);
        assert!(t.completed && t.completed_at.is_some());
        assert_eq!(t.previous_bucket, Some(Bucket::Soon));
        s.toggle(1);
        let t = s.tasks.iter().find(|t| t.id == 1).unwrap();
        assert_eq!(t.bucket, Bucket::Soon);
        assert!(!t.completed && t.completed_at.is_none() && t.previous_bucket.is_none());
        assert_eq!(ids(&s, Bucket::Soon), vec![1, 2]);
    }

    #[test]
    fn dragging_into_done_completes_and_out_reopens() {
        let mut s = state_with(1, Bucket::Later);
        s.move_to(1, Bucket::Done, 0);
        assert!(s.tasks[0].completed);
        assert_eq!(s.tasks[0].previous_bucket, Some(Bucket::Later));
        s.move_to(1, Bucket::Now, 0);
        assert!(!s.tasks[0].completed);
    }

    #[test]
    fn tidy_trims_and_drops_empty_titles() {
        let mut s = state_with(2, Bucket::Now);
        s.set_text(1, "  ");
        s.set_text(2, " keep ");
        s.set_desc(2, " note ");
        s.tidy();
        assert_eq!(s.tasks.len(), 1);
        assert_eq!(s.tasks[0].text, "keep");
        assert_eq!(s.tasks[0].desc, "note");
    }

    #[test]
    fn reads_electron_file_format() {
        let json = r#"{
          "tasks": [
            {"id": 5, "text": "A", "desc": "d", "bucket": "soon", "completed": false, "createdAt": 1728676800000},
            {"id": 2, "text": "B", "bucket": "done", "completed": true, "createdAt": 1, "completedAt": 2, "previousBucket": "now"}
          ],
          "selectedBucket": "later",
          "collapsed": {"done": true, "now": false},
          "nextId": 6
        }"#;
        let s: State = serde_json::from_str(json).unwrap();
        assert_eq!(s.tasks.len(), 2);
        assert_eq!(s.selected_bucket, Bucket::Later);
        assert_eq!(s.tasks[1].previous_bucket, Some(Bucket::Now));
        assert!(s.is_collapsed(Bucket::Done));
        assert_eq!(s.next_id, 6);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("qp-test-{}", now_ms()));
        let path = dir.join("planner-tasks.json");
        let mut s = state_with(2, Bucket::Now);
        s.toggle(1);
        s.widget = Some([-1200, 40, 300, 220]);
        save(&path, &s).unwrap();
        assert_eq!(load(&path), s);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn unreadable_file_is_backed_up_not_lost() {
        let dir = std::env::temp_dir().join(format!("qp-bad-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("planner-tasks.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load(&path), State::default());
        let backups = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(backups, 2);
        std::fs::remove_dir_all(dir).ok();
    }
}
