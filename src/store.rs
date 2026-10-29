//! Task state, the operations on it, and JSON persistence.
//!
//! The file format matches the old Electron app, so existing tasks load as-is.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
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
