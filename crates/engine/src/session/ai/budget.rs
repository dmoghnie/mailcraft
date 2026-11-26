//! A model's budgets, in tokens, for a calendar month and for a day: what each profile used this
//! month and today, as its server counted (or a token for every three bytes, where it doesn't
//! say), kept in `ai-usage.json`. A profile that has used either isn't asked: the model it falls
//! back to is, or none.

use std::collections::BTreeMap;

use mailcraft_model::{AiAnswer, dates};
use serde::{Deserialize, Serialize};

use super::{AiProfile, AiTask};
use crate::Session;

pub(crate) const AI_USAGE_DOC: &str = "ai-usage.json";

/// What each profile used in a month.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct AiUsage {
    /// The month counted, as months since the year 0 (year × 12 + month − 1), in local time.
    month: i64,
    /// The tokens each profile used in it, by its id.
    tokens: BTreeMap<String, u64>,
    /// The day counted (year × 10000 + month × 100 + day, in local time), and the tokens each
    /// profile used in it.
    day: i64,
    today: BTreeMap<String, u64>,
}

/// The profiles that have used a budget, by id: which one, as an error says it.
pub type Spent = BTreeMap<String, &'static str>;

/// A budget used, as an error says it: "{model} has used its …".
const MONTH_USED: &str = "monthly budget";
const DAY_USED: &str = "budget for today";

/// Bytes a token is taken for, where a server doesn't count them: few, so more are counted.
const BYTES_A_TOKEN: u64 = 3;

impl Session {
    /// This month, as [`AiUsage`] counts months.
    fn this_month(&self) -> i64 {
        let c = dates::civil(self.clock.now, &self.clock);
        c.year.saturating_mul(12).saturating_add(i64::from(c.month)).saturating_sub(1)
    }

    /// Today, as [`AiUsage`] counts days.
    fn this_day(&self) -> i64 {
        let c = dates::civil(self.clock.now, &self.clock);
        c.year.saturating_mul(10_000).saturating_add(i64::from(c.month) * 100).saturating_add(i64::from(c.day))
    }

    /// The tokens `profile` used this month.
    pub fn ai_used(&self, profile: &str) -> u64 {
        let u = &self.ai.usage;
        if u.month == self.this_month() { u.tokens.get(profile).copied().unwrap_or(0) } else { 0 }
    }

    /// The tokens `profile` used today.
    pub fn ai_used_today(&self, profile: &str) -> u64 {
        let u = &self.ai.usage;
        if u.day == self.this_day() { u.today.get(profile).copied().unwrap_or(0) } else { 0 }
    }

    /// The budget `profile` has used, this month's or today's, as an error says it.
    pub(crate) fn ai_budget_used(&self, profile: &AiProfile) -> Option<&'static str> {
        if profile.budget.is_some_and(|b| self.ai_used(&profile.id) >= b) {
            Some(MONTH_USED)
        } else if profile.daily.is_some_and(|b| self.ai_used_today(&profile.id) >= b) {
            Some(DAY_USED)
        } else {
            None
        }
    }

    /// Whether `profile` has used its budget this month, or its budget for today.
    pub fn ai_over_budget(&self, profile: &AiProfile) -> bool {
        self.ai_budget_used(profile).is_some()
    }

    /// The profiles that have used a budget, by id, and which.
    pub(crate) fn ai_spent(&self) -> Spent {
        self.prefs.ai.profiles.iter().filter_map(|p| self.ai_budget_used(p).map(|why| (p.id.clone(), why))).collect()
    }

    /// The profile `task` goes to with mail of `account`, as [`super::AiPrefs::route`] says,
    /// past those that have used their budget.
    pub(crate) fn ai_route(&self, task: AiTask, account: Option<&str>) -> Result<&AiProfile, String> {
        self.prefs.ai.route(task, account, &self.ai_spent())
    }

    /// An answer `profile` gave counted against its budget: its tokens as its server said, the
    /// question's (`asked`, in bytes) and the answer's, each estimated where it didn't.
    pub(super) fn ai_count(&mut self, profile: &str, asked: usize, a: &AiAnswer) {
        let bytes = |n: usize| u64::try_from(n).unwrap_or(u64::MAX).div_ceil(BYTES_A_TOKEN);
        let answered = a.text.len().saturating_add(a.thinking.len());
        let tokens = a.input_tokens.unwrap_or_else(|| bytes(asked)).saturating_add(a.output_tokens.unwrap_or_else(|| bytes(answered)));
        let (month, day) = (self.this_month(), self.this_day());
        let u = &mut self.ai.usage;
        if u.month != month {
            u.month = month;
            u.tokens.clear();
        }
        if u.day != day {
            u.day = day;
            u.today.clear();
        }
        let used = u.tokens.entry(profile.to_string()).or_insert(0);
        *used = used.saturating_add(tokens);
        let today = u.today.entry(profile.to_string()).or_insert(0);
        *today = today.saturating_add(tokens);
        self.ai.usage_changed = true;
    }

    /// What each profile used, as the last run left it: a damaged file is set aside, and the
    /// count begins again.
    pub(crate) fn load_ai_usage(&mut self) {
        let Some(store) = self.store.clone() else { return };
        match store.load_doc::<AiUsage>(AI_USAGE_DOC) {
            Ok(u) => self.ai.usage = u,
            Err(e) => log::warn!("what MailCraft's AI used this month couldn't be read: {e}"),
        }
    }

    /// What each profile used, saved once it changed.
    pub(crate) fn save_ai_usage(&mut self) {
        if !std::mem::take(&mut self.ai.usage_changed) {
            return;
        }
        if let Some(store) = &self.store
            && let Err(e) = store.save_doc(AI_USAGE_DOC, &self.ai.usage)
        {
            log::warn!("what MailCraft's AI used this month couldn't be saved: {e}");
        }
    }
}
