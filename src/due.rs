//! Due dates: calendar maths, the typed shorthand parser and the labels shown for them.
//! Everything here is pure; callers pass in today's date and the time.

pub const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
pub const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
    "November", "December",
];

/// A local calendar date.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    /// A date, or `None` if it doesn't exist (31 April, 29 February in a common year).
    pub fn new(year: i32, month: u32, day: u32) -> Option<Date> {
        let d = Date { year, month, day };
        ((1..=12).contains(&month) && day >= 1 && Date::from_days(d.days()) == d).then_some(d)
    }

    /// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
    pub fn days(self) -> i64 {
        let y = self.year as i64 - (self.month <= 2) as i64;
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = self.month as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + self.day as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn from_days(z: i64) -> Date {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
        Date { year: (yoe + era * 400 + (month <= 2) as i64) as i32, month, day }
    }

    pub fn add_days(self, n: i64) -> Date {
        Date::from_days(self.days() + n)
    }

    /// 0 is Sunday.
    pub fn weekday(self) -> u32 {
        (self.days() + 4).rem_euclid(7) as u32 // 1970-01-01 was a Thursday
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Due {
    pub date: Date,
    /// Minutes after midnight, when a time was given.
    pub time: Option<u32>,
}

impl Due {
    /// Reads the stored form, "2026-11-07" or "2026-11-07T15:00".
    pub fn parse(s: &str) -> Option<Due> {
        let (date, time) = match s.split_once('T') {
            Some((d, t)) => (d, Some(t)),
            None => (s, None),
        };
        let mut p = date.splitn(3, '-');
        let date = Date::new(p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.parse().ok()?)?;
        let time = match time {
            Some(t) => {
                let (h, m) = t.split_once(':')?;
                Some(hm(h.parse().ok()?, m.parse().ok()?)?)
            }
            None => None,
        };
        Some(Due { date, time })
    }

    pub fn stored(&self) -> String {
        let Date { year, month, day } = self.date;
        match self.time {
            Some(t) => format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}", t / 60, t % 60),
            None => format!("{year:04}-{month:02}-{day:02}"),
        }
    }

    /// Whether the due moment has passed; a date without a time is overdue from the next day.
    pub fn overdue(&self, today: Date, minute: u32) -> bool {
        self.date < today || (self.date == today && self.time.is_some_and(|t| t < minute))
    }

    pub fn urgency(&self, today: Date, minute: u32) -> Urgency {
        if self.overdue(today, minute) {
            return Urgency::Overdue;
        }
        match self.time {
            Some(t) if self.date == today && t - minute <= SOON_MINUTES => Urgency::Soon,
            _ => Urgency::Later,
        }
    }
}

/// How close a due moment is; labels change colour with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Urgency {
    Later,
    /// Within `SOON_MINUTES` of a timed task.
    Soon,
    Overdue,
}

pub const SOON_MINUTES: u32 = 60;

fn hm(h: u32, m: u32) -> Option<u32> {
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Splits trailing date words off a typed title: "Call mum tomorrow at 3pm" gives
/// ("Call mum", tomorrow 15:00). Only the end is read, so "Plan friday party" is left alone,
/// and at least one word always stays as the title.
pub fn split(title: &str, today: Date) -> (&str, Option<Due>) {
    let words: Vec<&str> = title.split_whitespace().collect();
    let mut n = words.len();
    // "3pm", or "3 pm" as two words.
    let time = match words.as_slice() {
        [.., h, ap] if ap.eq_ignore_ascii_case("am") || ap.eq_ignore_ascii_case("pm") => {
            parse_time(&format!("{h}{ap}")).map(|t| (t, 2))
        }
        [.., w] => parse_time(w).map(|t| (t, 1)),
        [] => None,
    };
    let time = time.map(|(t, k)| {
        n -= k;
        t
    });
    if time.is_some() {
        if n > 0 && words[n - 1].eq_ignore_ascii_case("at") {
            n -= 1;
        }
    }
    let mut date = None;
    for len in [2, 1] {
        if n > len {
            if let Some(d) = parse_date(&words[n - len..n], today) {
                date = Some(d);
                n -= len;
                break;
            }
        }
    }
    if (date.is_none() && time.is_none()) || n == 0 {
        return (title.trim(), None);
    }
    let end = words[n].as_ptr() as usize - title.as_ptr() as usize;
    (title[..end].trim(), Some(Due { date: date.unwrap_or(today), time }))
}

/// "3pm", "3:30pm", "12am" or "15:00", as minutes after midnight.
fn parse_time(word: &str) -> Option<u32> {
    let w = word.to_ascii_lowercase();
    let (num, pm) = match (w.strip_suffix("pm"), w.strip_suffix("am")) {
        (Some(n), _) => (n, Some(true)),
        (_, Some(n)) => (n, Some(false)),
        _ => (w.as_str(), None),
    };
    let (h, m): (u32, u32) = match num.split_once(':') {
        Some((h, m)) if m.len() == 2 => (h.parse().ok()?, m.parse().ok()?),
        None if pm.is_some() => (num.parse().ok()?, 0),
        _ => return None, // a bare number is not a time
    };
    match pm {
        Some(pm) if (1..=12).contains(&h) => hm(h % 12 + if pm { 12 } else { 0 }, m),
        Some(_) => None,
        None => hm(h, m),
    }
}

/// "today", "tomorrow", a weekday, "7 nov", "nov 7" or "7/11" (day first). Common short
/// forms and small typos ("tmrw", "tommorow", "wensday") count too.
fn parse_date(words: &[&str], today: Date) -> Option<Date> {
    let lower: Vec<String> = words.iter().map(|w| w.to_ascii_lowercase()).collect();
    match lower.as_slice() {
        [w] => match w.as_str() {
            "tdy" => Some(today),
            "tmr" | "tmrw" | "tmw" | "tomo" | "2moro" | "2morrow" | "2mrw" => Some(today.add_days(1)),
            w if close(w, "today") => Some(today),
            w if close(w, "tomorrow") => Some(today.add_days(1)),
            w => match weekday(w) {
                Some(wd) => Some(today.add_days(((wd + 7 - today.weekday()) % 7) as i64)),
                None => {
                    let (d, m) = w.split_once('/')?;
                    upcoming(d.parse().ok()?, m.parse().ok()?, today)
                }
            },
        },
        [a, b] => match (a.parse::<u32>(), month(b), month(a), b.parse::<u32>()) {
            (Ok(d), Some(m), ..) | (_, _, Some(m), Ok(d)) => upcoming(d, m, today),
            _ => None,
        },
        _ => None,
    }
}

/// The next date with this day and month, today included.
fn upcoming(day: u32, month: u32, today: Date) -> Option<Date> {
    [today.year, today.year + 1].into_iter().filter_map(|y| Date::new(y, month, day)).find(|d| *d >= today)
}

/// Full names, any prefix of three letters or more ("fri", "thurs"), or a near miss.
fn name_index(w: &str, names: &[&str]) -> Option<u32> {
    names
        .iter()
        .map(|n| n.to_ascii_lowercase())
        .position(|n| (w.len() >= 3 && n.starts_with(w)) || close(w, &n))
        .map(|i| i as u32)
}

fn weekday(w: &str) -> Option<u32> {
    if w == "weds" {
        return Some(3);
    }
    name_index(w, &DAYS)
}

/// Whether a typed word is `name` give or take a typo: one in names of 5 to 7 letters, two
/// in longer ones. Short words must match exactly, so "tom" is never "tomorrow".
fn close(w: &str, name: &str) -> bool {
    let slack = match name.len() {
        8.. => 2,
        5.. => 1,
        _ => 0,
    };
    w == name || (w.len() >= 4 && distance(w, name) <= slack)
}

/// Edit distance counting a swap of neighbours as one edit ("thrusday").
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = (a[i - 1] != b[j - 1]) as usize;
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

fn month(w: &str) -> Option<u32> {
    name_index(w, &MONTHS).map(|i| i + 1)
}

/// "today", "tomorrow", "yesterday", or "Sat 7 Nov" (with the year when not this year).
pub fn day_name(date: Date, today: Date) -> String {
    match date.days() - today.days() {
        0 => "today".into(),
        1 => "tomorrow".into(),
        -1 => "yesterday".into(),
        _ => {
            let s = format!(
                "{} {} {}",
                &DAYS[date.weekday() as usize][..3],
                date.day,
                &MONTHS[date.month as usize - 1][..3]
            );
            if date.year == today.year { s } else { format!("{s} {}", date.year) }
        }
    }
}

/// "3:00 pm".
pub fn time_name(minutes: u32) -> String {
    let (h, m) = (minutes / 60, minutes % 60);
    format!("{}:{m:02} {}", (h + 11) % 12 + 1, if h < 12 { "am" } else { "pm" })
}

/// The row label: "Due today, 3:00 pm", "Due Sat 7 Nov", "Due soon, 3:00 pm" or
/// "Overdue, yesterday".
pub fn label(due: Due, today: Date, minute: u32) -> String {
    let time = due.time.map(|t| format!(", {}", time_name(t))).unwrap_or_default();
    let day = day_name(due.date, today);
    match due.urgency(today, minute) {
        Urgency::Overdue => format!("Overdue, {day}{time}"),
        Urgency::Soon => format!("Due soon{time}"),
        Urgency::Later => format!("Due {day}{time}"),
    }
}

/// The widget's short label: "Overdue", today's time, or nothing.
pub fn short_label(due: Due, today: Date, minute: u32) -> String {
    match due.time {
        _ if due.overdue(today, minute) => "Overdue".into(),
        Some(t) if due.date == today => time_name(t),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::new(y, m, d).unwrap()
    }

    // Friday 30 October 2026.
    const TODAY: Date = Date { year: 2026, month: 10, day: 30 };

    #[test]
    fn calendar_maths_handles_edges() {
        assert_eq!(Date::from_days(0), date(1970, 1, 1));
        assert_eq!(TODAY.weekday(), 5);
        assert_eq!(date(2026, 12, 31).add_days(1), date(2027, 1, 1));
        assert_eq!(date(2028, 2, 28).add_days(1), date(2028, 2, 29));
        assert_eq!(Date::new(2026, 2, 29), None);
        assert_eq!(Date::new(2026, 4, 31), None);
        assert_eq!(Date::new(2026, 13, 1), None);
    }

    #[test]
    fn stored_form_round_trips() {
        for s in ["2026-11-07", "2026-11-07T15:00", "2027-01-03T00:05"] {
            assert_eq!(Due::parse(s).unwrap().stored(), s);
        }
        assert_eq!(Due::parse("2026-02-30"), None);
        assert_eq!(Due::parse("soon"), None);
    }

    #[test]
    fn split_reads_trailing_date_words() {
        let due = |y, m, d, time| Some(Due { date: date(y, m, d), time });
        let cases = [
            ("Essay fri", "Essay", due(2026, 10, 30, None)),
            ("Essay Monday", "Essay", due(2026, 11, 2, None)),
            ("Call mum tomorrow at 3pm", "Call mum", due(2026, 10, 31, Some(900))),
            ("Meeting 15:30", "Meeting", due(2026, 10, 30, Some(930))),
            ("Meeting at 3:30PM", "Meeting", due(2026, 10, 30, Some(930))),
            ("Sleep 12am", "Sleep", due(2026, 10, 30, Some(0))),
            ("Lunch thurs 12:30pm", "Lunch", due(2026, 11, 5, Some(750))),
            ("Trip 7 nov", "Trip", due(2026, 11, 7, None)),
            ("Trip November 7", "Trip", due(2026, 11, 7, None)),
            ("Pay rent 7/11", "Pay rent", due(2026, 11, 7, None)),
            ("Gift 3/1", "Gift", due(2027, 1, 3, None)),
            ("Old 29 oct", "Old", due(2027, 10, 29, None)),
            ("Call 3 pm", "Call", due(2026, 10, 30, Some(900))),
            ("Call mum tommorow", "Call mum", due(2026, 10, 31, None)),
            ("Call mum tmr 3 pm", "Call mum", due(2026, 10, 31, Some(900))),
            ("Gym tday", "Gym", due(2026, 10, 30, None)),
            ("Gym weds", "Gym", due(2026, 11, 4, None)),
            ("Lunch wensday", "Lunch", due(2026, 11, 4, None)),
            ("Lunch thrusday", "Lunch", due(2026, 11, 5, None)),
            ("Trip 7 novmber", "Trip", due(2026, 11, 7, None)),
        ];
        for (typed, title, want) in cases {
            assert_eq!(split(typed, TODAY), (title, want), "{typed}");
        }
    }

    #[test]
    fn split_leaves_other_titles_alone() {
        for typed in [
            "Plan friday party", "friday", "3pm", "Buy 7 apples", "Room 13pm", "Read 31/2", "Call at",
            "Call tom", "Call Monica", "Buy candy", "Clean fridge", "Save money",
        ] {
            assert_eq!(split(typed, TODAY), (typed, None), "{typed}");
        }
    }

    #[test]
    fn labels_read_naturally() {
        let at = |d, time| Due { date: d, time };
        assert_eq!(label(at(TODAY, Some(900)), TODAY, 600), "Due today, 3:00 pm");
        assert_eq!(label(at(date(2026, 11, 7), None), TODAY, 600), "Due Sat 7 Nov");
        assert_eq!(label(at(date(2027, 1, 3), None), TODAY, 600), "Due Sun 3 Jan 2027");
        assert_eq!(label(at(date(2026, 10, 29), None), TODAY, 600), "Overdue, yesterday");
        assert_eq!(label(at(TODAY, None), TODAY, 1439), "Due today");
        assert_eq!(label(at(TODAY, Some(900)), TODAY, 900 - SOON_MINUTES), "Due soon, 3:00 pm");
        assert_eq!(at(TODAY, Some(900)).urgency(TODAY, 900 - SOON_MINUTES - 1), Urgency::Later);
        assert_eq!(at(TODAY, None).urgency(TODAY, 1439), Urgency::Later);
        assert_eq!(short_label(at(TODAY, Some(900)), TODAY, 600), "3:00 pm");
        assert_eq!(short_label(at(TODAY, Some(900)), TODAY, 901), "Overdue");
        assert_eq!(short_label(at(date(2026, 10, 31), None), TODAY, 600), "");
        assert_eq!(time_name(0), "12:00 am");
        assert_eq!(time_name(12 * 60 + 5), "12:05 pm");
    }
}
