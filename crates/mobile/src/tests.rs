use super::*;
use crux_core::Core;
use mailcraft_engine::store::Store;

const NOW: i64 = 1_795_330_000;

/// The app on the sample mailbox, in memory.
fn sample() -> Core<Mail> {
    let store = Store::memory();
    assert!(mailcraft_engine::sample::seed_if_empty(&store, NOW).unwrap());
    let core: Core<Mail> = Core::new();
    core.process_event(Event::Opened(Box::new(Session::open(Some(store), Clock::new(NOW, 0)))));
    core
}

fn act(core: &Core<Mail>, json: &str) -> Vec<Effect> {
    let a: Action = serde_json::from_str(json).unwrap();
    core.process_event(Event::Shell(a))
}

fn jobs(effects: Vec<Effect>) -> Vec<Job> {
    effects
        .into_iter()
        .filter_map(|e| match e {
            Effect::Engine(r) => Some(r.operation.0),
            Effect::Render(_) => None,
        })
        .flatten()
        .collect()
}

#[test]
fn opens_on_the_folders_with_the_accounts() {
    let core = sample();
    let v = core.view();
    assert!(v.ready);
    assert_eq!(v.screen, Screen::Folders);
    assert_eq!(v.accounts.len(), 1);
    assert_eq!(v.accounts[0].email, "riley@studio.example");
    assert!(v.accounts[0].folders.iter().any(|f| f["role"] == "inbox"));
}

#[test]
fn before_the_cache_opens_nothing_runs() {
    let core: Core<Mail> = Core::new();
    act(&core, r#"{"type":"openFolder","account":"sample","folder":"INBOX"}"#);
    let v = core.view();
    assert!(!v.ready);
    assert_eq!(v.screen, Screen::Folders);
}

#[test]
fn a_folder_lists_its_messages_and_a_message_opens() {
    let core = sample();
    act(&core, r#"{"type":"openFolder","account":"sample","folder":"INBOX"}"#);
    let v = core.view();
    assert_eq!(v.screen, Screen::Messages);
    assert_eq!(v.title, "Inbox");
    assert_eq!(v.total, 7);
    assert_eq!(v.messages.len(), 7);
    // Only the row's fields a phone shows.
    assert!(v.messages[0].get("senderWarning").is_none());
    let key = v.messages.iter().find(|m| m["subject"] == "Storyboard review on Thursday").unwrap()["key"].as_str().unwrap().to_string();

    act(&core, &format!(r#"{{"type":"openMessage","key":"{key}"}}"#));
    let v = core.view();
    assert_eq!(v.screen, Screen::Message);
    let m = v.message.unwrap();
    assert_eq!(m["subject"], "Storyboard review on Thursday");
    assert!(m["text"].as_str().unwrap().starts_with("Hi Riley"));

    act(&core, r#"{"type":"back"}"#);
    assert_eq!(core.view().screen, Screen::Messages);
    assert!(core.view().message.is_none());
    act(&core, r#"{"type":"back"}"#);
    assert_eq!(core.view().screen, Screen::Folders);
}

#[test]
fn a_reply_is_written_and_back_keeps_it_in_drafts() {
    let core = sample();
    act(&core, r#"{"type":"openFolder","account":"sample","folder":"INBOX"}"#);
    let key = core.view().messages.iter().find(|m| m["subject"] == "Storyboard review on Thursday").unwrap()["key"].as_str().unwrap().to_string();
    act(&core, &format!(r#"{{"type":"openMessage","key":"{key}"}}"#));
    act(&core, r#"{"type":"compose","kind":"reply"}"#);
    let v = core.view();
    assert_eq!(v.screen, Screen::Compose);
    let c = v.compose.unwrap();
    assert_eq!(c.subject, "Re: Storyboard review on Thursday");
    assert!(c.to.contains("zoe@studio.example"));

    act(&core, r#"{"type":"edit","body":"Thursday works."}"#);
    assert!(core.view().compose.unwrap().body.starts_with("Thursday works."));

    act(&core, r#"{"type":"back"}"#);
    let v = core.view();
    assert_eq!(v.screen, Screen::Message, "back to the message replied to");
    assert!(v.compose.is_none());
}

#[test]
fn an_account_added_with_its_servers_starts_and_its_password_stays_out_of_the_view() {
    let core = sample();
    act(&core, r#"{"type":"startAdding"}"#);
    assert_eq!(core.view().screen, Screen::AddAccount);
    let e = act(
        &core,
        r#"{"type":"addAccount","email":"zoe@example.org","password":"pw-not-shown","imap":"mail.example.org:993","smtp":"mail.example.org:465"}"#,
    );
    assert!(jobs(e).iter().any(|j| matches!(j, Job::Start { account, .. } if account.imap.host == "mail.example.org")));
    let v = core.view();
    assert_eq!(v.screen, Screen::Folders);
    assert!(v.accounts.iter().any(|a| a.email == "zoe@example.org"));
    assert!(!serde_json::to_string(&v).unwrap().contains("pw-not-shown"));
}

#[test]
fn an_account_without_servers_is_looked_up_first() {
    let core = sample();
    let e = act(&core, r#"{"type":"addAccount","email":"zoe@example.org","password":"pw"}"#);
    assert!(jobs(e).iter().any(|j| matches!(j, Job::Discover { email } if email == "zoe@example.org")));
    assert!(core.view().adding);
    assert_eq!(core.view().accounts.len(), 1, "nothing added before the answer");
}

#[test]
fn half_a_form_is_said_and_adds_nothing() {
    let core = sample();
    act(&core, r#"{"type":"addAccount","email":"zoe@example.org","password":"pw","imap":"mail.example.org:993"}"#);
    assert!(core.view().notice.unwrap().contains("both servers"));
    act(&core, r#"{"type":"dismissNotice"}"#);
    act(&core, r#"{"type":"addAccount","email":"zoe@example.org","password":"pw","imap":"mail.example.org","smtp":"x:y"}"#);
    assert!(core.view().notice.unwrap().contains("host:port"));
    act(&core, r#"{"type":"addAccount","email":"","password":""}"#);
    assert!(core.view().notice.is_some());
    assert_eq!(core.view().accounts.len(), 1);
}

#[test]
fn junk_actions_are_refused_or_said_never_a_crash() {
    let core = sample();
    for a in [
        r#"{"type":"openFolder","account":"nobody","folder":"nowhere"}"#,
        r#"{"type":"openMessage","key":"not a key"}"#,
        r#"{"type":"row","key":"sample:9:9:INBOX","action":"delete"}"#,
        r#"{"type":"row","key":"x","action":"explode"}"#,
        r#"{"type":"compose","kind":"telegram"}"#,
        r#"{"type":"compose","kind":"reply"}"#,
        r#"{"type":"edit","to":"a"}"#,
        r#"{"type":"send"}"#,
        r#"{"type":"discard"}"#,
        r#"{"type":"removeAccount","account":"nobody"}"#,
        r#"{"type":"search","query":""}"#,
        r#"{"type":"loadMore"}"#,
        r#"{"type":"refresh"}"#,
        r#"{"type":"back"}"#,
    ] {
        act(&core, a);
        let _ = serde_json::to_string(&core.view()).unwrap();
    }
    assert!(serde_json::from_str::<Action>(r#"{"type":"nonsense"}"#).is_err());
}
