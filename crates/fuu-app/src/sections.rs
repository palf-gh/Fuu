//! Date sections in the message list (今日, 昨日, 一昨日, 今月, one per month for
//! the past twelve months, then さらに前). Each row carries its section name;
//! the first row of a run shows the header, and a collapsed section hides its
//! rows but keeps the header.

use std::cell::RefCell;
use std::collections::HashSet;

use chrono::{Datelike, Local, NaiveDate, TimeZone};
use slint::{Model, VecModel};

use crate::MessageRow;

/// Months before this one that get a section of their own.
const MONTHS: i32 = 12;

thread_local! {
    /// Sections the user collapsed (kept across folder switches this session).
    static COLLAPSED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// The section for a message dated `timestamp`, seen from `today` (local).
pub fn date_section(timestamp: i64, today: NaiveDate) -> String {
    let Some(date) = Local.timestamp_opt(timestamp, 0).single().map(|d| d.date_naive()) else {
        return "さらに前".into();
    };
    section_for(date, today)
}

/// The section for `date`; future dates (clock skew) count as today.
pub fn section_for(date: NaiveDate, today: NaiveDate) -> String {
    match (today - date).num_days() {
        ..=0 => return "今日".into(),
        1 => return "昨日".into(),
        2 => return "一昨日".into(),
        _ => {}
    }
    let months_back = (today.year() * 12 + today.month0() as i32) - (date.year() * 12 + date.month0() as i32);
    match months_back {
        0 => "今月".into(),
        1..=MONTHS if date.year() == today.year() => format!("{}月", date.month()),
        1..=MONTHS => format!("{}年{}月", date.year(), date.month()),
        _ => "さらに前".into(),
    }
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// Set `header` on the first row of each run and `collapsed` from the user's
/// choices. `before` is the section of the row preceding `rows` (streaming).
pub fn mark(rows: &mut [MessageRow], before: Option<&str>) {
    COLLAPSED.with(|c| {
        let c = c.borrow();
        let mut prev = before.map(str::to_owned);
        for row in rows {
            row.header = prev.as_deref() != Some(row.section.as_str());
            row.collapsed = c.contains(row.section.as_str());
            prev = Some(row.section.to_string());
        }
    });
}

/// Flip a section open or closed; returns whether it is now collapsed.
pub fn toggle(model: &VecModel<MessageRow>, section: &str) -> bool {
    let collapsed = COLLAPSED.with(|c| {
        let mut c = c.borrow_mut();
        if c.remove(section) {
            false
        } else {
            c.insert(section.to_owned());
            true
        }
    });
    for i in 0..model.row_count() {
        let Some(row) = model.row_data(i) else { continue };
        if row.section == section && row.collapsed != collapsed {
            model.set_row_data(i, MessageRow { collapsed, ..row });
        }
    }
    collapsed
}

/// After removing a row at `index`: the next row takes over its header.
pub fn removed(model: &VecModel<MessageRow>, index: usize, was_header: bool) {
    if !was_header {
        return;
    }
    if let Some(next) = model.row_data(index) {
        let prev_same = index > 0 && model.row_data(index - 1).is_some_and(|p| p.section == next.section);
        if !next.header && !prev_same {
            model.set_row_data(index, MessageRow { header: true, ..next });
        }
    }
}

/// After a removal at `from`: rows from there on move up one place.
pub fn renumber(model: &VecModel<MessageRow>, from: usize) {
    for i in from..model.row_count() {
        if let Some(row) = model.row_data(i) {
            if row.index != i as i32 {
                model.set_row_data(i, MessageRow { index: i as i32, ..row });
            }
        }
    }
}

/// The nearest visible row from `from` in direction `dir` (±1), or `from`.
pub fn step(model: &impl Model<Data = MessageRow>, from: i32, dir: i32) -> i32 {
    let count = model.row_count() as i32;
    let mut i = from + dir;
    while (0..count).contains(&i) {
        if model.row_data(i as usize).is_some_and(|r| !r.collapsed) {
            return i;
        }
        i += dir;
    }
    from
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn recent_days() {
        let today = d(2026, 11, 19);
        assert_eq!(section_for(d(2026, 11, 19), today), "今日");
        assert_eq!(section_for(d(2026, 11, 20), today), "今日");
        assert_eq!(section_for(d(2026, 11, 18), today), "昨日");
        assert_eq!(section_for(d(2026, 11, 17), today), "一昨日");
        assert_eq!(section_for(d(2026, 11, 16), today), "今月");
        assert_eq!(section_for(d(2026, 11, 1), today), "今月");
    }

    #[test]
    fn months_and_older() {
        let today = d(2026, 11, 19);
        assert_eq!(section_for(d(2026, 10, 31), today), "10月");
        assert_eq!(section_for(d(2026, 1, 5), today), "1月");
        assert_eq!(section_for(d(2025, 12, 5), today), "2025年12月");
        assert_eq!(section_for(d(2025, 11, 30), today), "2025年11月");
        assert_eq!(section_for(d(2025, 10, 31), today), "さらに前");
    }

    #[test]
    fn yesterday_wins_over_last_month() {
        let today = d(2026, 12, 1);
        assert_eq!(section_for(d(2026, 11, 30), today), "昨日");
        assert_eq!(section_for(d(2026, 11, 29), today), "一昨日");
        assert_eq!(section_for(d(2026, 11, 28), today), "11月");
    }

    fn row(section: &str) -> MessageRow {
        MessageRow { section: section.into(), ..Default::default() }
    }

    #[test]
    fn headers_follow_runs() {
        let mut rows = vec![row("今日"), row("今日"), row("昨日"), row("10月")];
        mark(&mut rows, None);
        assert_eq!(rows.iter().map(|r| r.header).collect::<Vec<_>>(), [true, false, true, true]);
        let mut more = vec![row("10月"), row("9月")];
        mark(&mut more, Some("10月"));
        assert_eq!(more.iter().map(|r| r.header).collect::<Vec<_>>(), [false, true]);
    }

    #[test]
    fn collapsing_hides_rows_and_steps_skip_them() {
        let mut rows = vec![row("今日"), row("昨日"), row("昨日"), row("10月")];
        mark(&mut rows, None);
        let model = VecModel::from(rows);
        assert!(toggle(&model, "昨日"));
        assert!(model.row_data(1).unwrap().collapsed && model.row_data(2).unwrap().collapsed);
        assert_eq!(step(&model, 0, 1), 3);
        assert_eq!(step(&model, 3, -1), 0);
        assert_eq!(step(&model, 3, 1), 3);
        assert!(!toggle(&model, "昨日"));
        assert_eq!(step(&model, 0, 1), 1);
    }

    #[test]
    fn removing_a_header_promotes_the_next_row() {
        let mut rows = vec![row("今日"), row("昨日"), row("昨日")];
        mark(&mut rows, None);
        let model = VecModel::from(rows);
        model.remove(1);
        removed(&model, 1, true);
        assert!(model.row_data(1).unwrap().header);
    }
}
