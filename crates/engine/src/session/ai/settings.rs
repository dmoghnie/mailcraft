//! What the person set up for MailCraft's AI: the models it may ask (profiles), which one does
//! each kind of work, and how far each account's mail may go. Kept with the other settings; a
//! profile's key is in the keychain, never here.

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use mailcraft_model::{AiEffort, AiEndpoint};
use serde::{Deserialize, Serialize};

/// A kind of work a profile does; each AI feature names the one it needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiTask {
    /// Writing, summarising, research: the best model set up.
    Chat,
    /// Small, quick choices: triage, routing, whether a message needs an answer.
    Decision,
}

impl AiTask {
    pub const ALL: [AiTask; 2] = [AiTask::Chat, AiTask::Decision];

    pub fn code(self) -> &'static str {
        match self {
            AiTask::Chat => "chat",
            AiTask::Decision => "decision",
        }
    }

    pub fn from_code(s: &str) -> Option<AiTask> {
        AiTask::ALL.into_iter().find(|t| t.code() == s)
    }
}

/// How far an account's mail may go to a model. An account not given one sends none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiReach {
    /// To models on this computer or its own network alone ([`AiProfile::local`]).
    Local,
    /// To any model set up, a cloud provider's too.
    Any,
}

/// A model MailCraft may ask, as the person named it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AiProfile {
    /// `p1`, `p2`…: also the name its key is kept under in the keychain.
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub endpoint: AiEndpoint,
    /// A key is kept in the keychain for it.
    pub has_key: bool,
    /// The longest answer it's asked for, in tokens.
    pub max_tokens: u32,
    /// What it takes in at once, in tokens (0: [`CONTEXT_AT_FIRST`](super::CONTEXT_AT_FIRST)):
    /// the assistant's chats are fitted into it.
    #[serde(skip_serializing_if = "is_zero")]
    pub context: u32,
    /// The profile asked instead when this one can't be reached or refuses (one on this
    /// computer, say).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
    /// Its temperature, in hundredths (0 to 200); none: the model's own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<u16>,
    /// How much it thinks before it answers; none: as the model does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<AiEffort>,
    /// The most tokens it may use in a calendar month, its questions' and answers'; none: no
    /// limit. Past it, the model it falls back to is asked instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<u64>,
    /// The assistant may run scripts it writes with it (code mode): off until the person turns
    /// it on.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub scripts: bool,
    /// Not a model but an outside agent, whose script reads every account (`script.run`), as
    /// the agent's own commands do: made in code alone, never read from the settings.
    #[serde(skip)]
    pub(crate) agent: bool,
}

impl AiProfile {
    /// What it takes in at once, in tokens: its own, or [`CONTEXT_AT_FIRST`](super::CONTEXT_AT_FIRST).
    pub fn takes_in(&self) -> u32 {
        if self.context > 0 { self.context } else { super::CONTEXT_AT_FIRST }
    }

    /// Whether its server is on this computer or its own network: a loopback, private or
    /// link-local address, `localhost`, or a `.local` name. Anything else is taken for a third
    /// party's, whatever it's called.
    pub fn local(&self) -> bool {
        local_url(&self.endpoint.url)
    }
}

/// The longest the person can keep chats for, in days ([`AiPrefs::chats_days`]): a hundred years.
pub const CHATS_DAYS_MOST: u32 = 36_500;

/// MailCraft's AI as set up. Empty at first: nothing is asked until the person adds a model and
/// lets an account's mail go to it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AiPrefs {
    pub profiles: Vec<AiProfile>,
    /// The profile each kind of work goes to (none named: the first profile).
    pub uses: BTreeMap<AiTask, String>,
    /// How far each account's mail may go, by account id.
    pub accounts: BTreeMap<String, AiReach>,
    /// Models on this computer or its network alone, whatever an account allows: nothing goes
    /// to a cloud provider, a test included.
    pub local_only: bool,
    /// Chats with the assistant are deleted once they're this many days old (since they last
    /// changed), with the mail they hold; 0: kept until the person deletes them.
    #[serde(skip_serializing_if = "is_zero")]
    pub chats_days: u32,
    /// The MCP servers whose tools its scripts may call (`servers.rs`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mcp: Vec<super::McpServer>,
}

impl AiPrefs {
    pub fn is_empty(&self) -> bool {
        *self == AiPrefs::default()
    }

    pub fn profile(&self, id: &str) -> Option<&AiProfile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    /// How far mail of `account` may go (`None`: no mail goes, as in a test), or `None` when
    /// none of its mail may go to a model.
    pub fn reach(&self, account: Option<&str>) -> Option<AiReach> {
        let reach = match account {
            Some(a) => *self.accounts.get(a)?,
            None => AiReach::Any,
        };
        Some(if self.local_only { AiReach::Local } else { reach })
    }

    /// Whether what MailCraft's AI holds of `account`'s mail (none: no mail) may go to MCP
    /// server `server`: one on this computer or its network wherever mail may go to a model,
    /// one elsewhere only where it may go to any.
    pub fn allows_server(&self, server: &super::McpServer, account: Option<&str>) -> bool {
        match self.reach(account) {
            Some(AiReach::Any) => true,
            Some(AiReach::Local) => server.local(),
            None => false,
        }
    }

    /// Whether `profile` may be asked about mail of `account`.
    pub fn allows(&self, profile: &AiProfile, account: Option<&str>) -> bool {
        if profile.agent {
            return true;
        }
        match self.reach(account) {
            Some(AiReach::Any) => true,
            Some(AiReach::Local) => profile.local(),
            None => false,
        }
    }

    /// Whether `profile` may be asked about mail of every one of `accounts` (none: a question
    /// that holds no mail).
    pub fn allows_all(&self, profile: &AiProfile, accounts: &[String]) -> bool {
        if accounts.is_empty() { self.allows(profile, None) } else { accounts.iter().all(|a| self.allows(profile, Some(a))) }
    }

    /// `from`, or the first along its fallbacks allowed mail of every one of `accounts`, past
    /// those in `spent` (that have used their budget).
    pub fn allowed_for_all<'a>(&'a self, from: &'a AiProfile, accounts: &[String], spent: &BTreeSet<String>) -> Option<&'a AiProfile> {
        let mut seen: Vec<&str> = Vec::new();
        let mut at = Some(from);
        while let Some(p) = at.filter(|p| !seen.contains(&p.id.as_str())) {
            if self.allows_all(p, accounts) && !spent.contains(&p.id) {
                return Some(p);
            }
            seen.push(&p.id);
            at = p.fallback.as_deref().and_then(|id| self.profile(id));
        }
        None
    }

    /// The profile `task` goes to for mail of `account`: the one named for it, or the first,
    /// or else along their fallbacks the first that's allowed and not in `spent` (that have used
    /// their budget). Why none, when none is.
    pub fn route(&self, task: AiTask, account: Option<&str>, spent: &BTreeSet<String>) -> Result<&AiProfile, String> {
        if self.reach(account).is_none() {
            return Err("MailCraft's AI is off for this account: File › Options › AI lets its mail go to a model".into());
        }
        let first = self.uses.get(&task).and_then(|id| self.profile(id)).or_else(|| self.profiles.first());
        let Some(first) = first else { return Err("no model is set up: add one in File › Options › AI".into()) };
        self.allowed_from(first, account, spent).ok_or_else(|| {
            if spent.contains(&first.id) {
                format!("{} has used its monthly budget, and no model it falls back to may have this mail", first.name)
            } else {
                format!("{} isn't on this computer or its network, and this mail goes only to a model that is", first.name)
            }
        })
    }

    /// `from`, or the first allowed along its fallbacks (each profile tried once), past those in
    /// `spent`.
    pub fn allowed_from<'a>(&'a self, from: &'a AiProfile, account: Option<&str>, spent: &BTreeSet<String>) -> Option<&'a AiProfile> {
        let mut seen: Vec<&str> = Vec::new();
        let mut at = Some(from);
        while let Some(p) = at.filter(|p| !seen.contains(&p.id.as_str())) {
            if self.allows(p, account) && !spent.contains(&p.id) {
                return Some(p);
            }
            seen.push(&p.id);
            at = p.fallback.as_deref().and_then(|id| self.profile(id));
        }
        None
    }
}

/// Whether a web address's host is this computer or its own network ([`AiProfile::local`]).
pub fn local_url(url: &str) -> bool {
    let rest = url.trim();
    let rest =
        ["https://", "http://"].iter().find_map(|s| rest.get(..s.len()).filter(|h| h.eq_ignore_ascii_case(s)).and_then(|_| rest.get(s.len()..)));
    let Some(rest) = rest else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return false;
    }
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => authority.rsplit_once(':').map_or(authority, |(h, _)| h),
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        Ok(IpAddr::V6(v6)) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
            None => v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local(),
        },
        Err(_) => false,
    }
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}
