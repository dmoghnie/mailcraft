//! Meeting times proposed in the message being written: the person's free times in their work
//! hours over the next week (none of their own calendars taken, tentative counting as taken),
//! given to the model that writes the message, which proposes a few of them. Only when the person
//! asks (`compose.proposeTimes`); a colleague's calendar never goes.

use mailcraft_model::dates;
use mailcraft_model::ical::ShowAs;
use mailcraft_model::pim::work_hours::{merged, outside};

use crate::{CmdError, Session};

/// How far ahead free times are looked for.
const DAYS_AHEAD: i64 = 7;
/// The most free times given.
const TIMES_MOST: usize = 12;
/// Said when there's none.
const NO_FREE_TIME: &str = "there's no free time in your work hours in the next week";

impl Session {
    /// The person's free times at least `length` seconds long in their work hours over the next
    /// week, each starting on the half hour: none of their own calendars has anything there
    /// that keeps them busy.
    pub(crate) fn my_free_times(&self, length: i64) -> Vec<(i64, i64)> {
        let from = self.on_the_half_hour(self.clock.now, true);
        let to = from.saturating_add(DAYS_AHEAD * 86_400);
        let work = self.work_spans(from, to);
        let taken: Vec<(i64, i64)> = self
            .agenda_where(from, to, |account, cal| !self.pim.get(account).is_some_and(|p| p.is_theirs(&cal.id)))
            .into_iter()
            .filter(|it| !it.task && it.status != "CANCELLED" && !matches!(it.show_as, ShowAs::Free | ShowAs::WorkingElsewhere))
            .map(|it| (it.start, it.end.max(it.start.saturating_add(1))))
            .collect();
        let taken = merged(taken);
        work.iter()
            .flat_map(|&(a, b)| outside(&taken, a, b))
            .filter_map(|(a, b)| {
                let a = self.on_the_half_hour(a, true);
                (b.saturating_sub(a) >= length).then_some((a, b))
            })
            .take(TIMES_MOST)
            .collect()
    }

    /// What the model writing the message is asked to say, proposing times for a meeting of
    /// `minutes`, and the person's free times it picks from.
    pub(super) fn times_asked(&self, minutes: u16) -> Result<String, CmdError> {
        let free = self.my_free_times(i64::from(minutes) * 60);
        if free.is_empty() {
            return Err(CmdError::Failed(NO_FREE_TIME.into()));
        }
        let list: Vec<String> = free
            .iter()
            .map(|&(a, b)| {
                let day = self.clock.local_day(a);
                format!("- {} {}, {} to {}", dates::weekday_long(day), dates::day_string(day), dates::hm(a, &self.clock), dates::hm(b, &self.clock))
            })
            .collect();
        Ok(format!(
            "propose two or three of the person's free times below for a meeting of {minutes} minutes, each as a day and a start time, and answer anything else the message asks only as far as it tells you. Their free times, in their own time zone:\n{}",
            list.join("\n")
        ))
    }
}
