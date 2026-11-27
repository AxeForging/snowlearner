//! The practice streak (spec 006 R13): consecutive local days with at least
//! one right answer. It ends today, or yesterday while today has none yet, so
//! it stays alive until midnight. Pure.

use chrono::NaiveDate;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Streak {
    /// Days in a row, 0 when neither today nor yesterday counts.
    pub days: u32,
    /// Today already has a right answer; false while the streak waits for one.
    pub done_today: bool,
}

/// The streak on `today` from the days with a right answer (any order,
/// repeats allowed).
pub fn streak(days: &[NaiveDate], today: NaiveDate) -> Streak {
    let has = |d: NaiveDate| days.contains(&d);
    let done_today = has(today);
    let Some(mut day) = (if done_today { Some(today) } else { today.pred_opt().filter(|d| has(*d)) }) else {
        return Streak::default();
    };
    let mut count = 0;
    while has(day) {
        count += 1;
        let Some(before) = day.pred_opt() else { break };
        day = before;
    }
    Streak { days: count, done_today }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    #[test]
    fn no_practice_is_no_streak() {
        assert_eq!(streak(&[], day(27)), Streak { days: 0, done_today: false });
    }

    #[test]
    fn today_alone_counts_one() {
        assert_eq!(streak(&[day(27)], day(27)), Streak { days: 1, done_today: true });
    }

    #[test]
    fn yesterday_keeps_it_alive_until_midnight() {
        assert_eq!(streak(&[day(25), day(26)], day(27)), Streak { days: 2, done_today: false });
    }

    #[test]
    fn a_missed_day_resets_it() {
        assert_eq!(streak(&[day(24), day(25)], day(27)), Streak::default(), "nothing yesterday nor today");
        assert_eq!(streak(&[day(23), day(25), day(26), day(27)], day(27)).days, 3, "the gap on the 24th stops it");
    }

    #[test]
    fn repeats_and_order_do_not_matter() {
        let s = streak(&[day(27), day(26), day(27), day(26), day(25)], day(27));
        assert_eq!(s, Streak { days: 3, done_today: true });
    }
}
