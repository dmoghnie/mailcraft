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
    assert!(v.accounts[0].folders.iter().any(|f| f.role == "inbox"));
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

/// The check job an action asked for: its id.
fn check_id(effects: Vec<Effect>) -> u64 {
    jobs(effects).iter().find_map(|j| if let Job::CheckSignIn { id, .. } = j { Some(*id) } else { None }).expect("a sign-in check")
}

const ZOE: &str =
    r#"{"type":"addAccount","email":"zoe@example.org","password":"pw-not-shown","imap":"mail.example.org:993","smtp":"mail.example.org:465"}"#;

#[test]
fn an_account_is_added_once_its_sign_in_is_taken_and_its_password_stays_out_of_the_view() {
    let core = sample();
    act(&core, r#"{"type":"startAdding"}"#);
    assert_eq!(core.view().screen, Screen::AddAccount);
    let id = check_id(act(&core, ZOE));
    let v = core.view();
    assert_eq!((v.adding, v.adding_step.as_deref()), (true, Some("checking")));
    assert_eq!(v.accounts.len(), 1, "nothing added before the servers answer");

    let e = core.process_event(Event::Engine(vec![mailcraft_engine::model::Event::SignInChecked { id, failure: None }]));
    assert!(jobs(e).iter().any(|j| matches!(j, Job::Start { account, .. } if account.imap.host == "mail.example.org")));
    let v = core.view();
    assert_eq!(v.screen, Screen::Folders);
    assert!(!v.adding);
    assert!(v.accounts.iter().any(|a| a.email == "zoe@example.org"));
    assert!(!serde_json::to_string(&v).unwrap().contains("pw-not-shown"));
}

#[test]
fn a_refused_sign_in_says_which_server_and_why_and_can_be_added_anyway() {
    let core = sample();
    act(&core, r#"{"type":"startAdding"}"#);
    let id = check_id(act(&core, ZOE));
    let failure = mailcraft_engine::model::SignInFailure {
        server: "mail.example.org".into(),
        outgoing: true,
        error: "535 authentication failed".into(),
        certificate: None,
    };
    core.process_event(Event::Engine(vec![mailcraft_engine::model::Event::SignInChecked { id, failure: Some(Box::new(failure)) }]));
    let v = core.view();
    assert_eq!(v.screen, Screen::AddAccount);
    assert!(v.can_add_anyway && !v.adding);
    let notice = v.notice.unwrap();
    assert!(notice.contains("outgoing server mail.example.org") && notice.contains("535"), "{notice}");
    assert_eq!(v.accounts.len(), 1);

    act(&core, r#"{"type":"addAnyway"}"#);
    let v = core.view();
    assert_eq!(v.screen, Screen::Folders);
    assert!(!v.can_add_anyway);
    assert!(v.accounts.iter().any(|a| a.email == "zoe@example.org"));
    act(&core, r#"{"type":"addAnyway"}"#);
    assert_eq!(core.view().accounts.len(), 2, "added once");
}

#[test]
fn an_answer_to_an_older_check_isnt_taken_for_this_one() {
    let core = sample();
    let first = check_id(act(&core, ZOE));
    let second = check_id(act(&core, &ZOE.replace("zoe@", "sam@")));
    assert_ne!(first, second);
    core.process_event(Event::Engine(vec![mailcraft_engine::model::Event::SignInChecked { id: first, failure: None }]));
    let v = core.view();
    assert_eq!(v.adding_step.as_deref(), Some("checking"), "still waiting for sam's");
    assert_eq!(v.accounts.len(), 1);
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
        r#"{"type":"row","key":"x","action":"archive"}"#,
        r#"{"type":"collapse","account":""}"#,
        r#"{"type":"collapse","account":"nobody","folder":"nowhere"}"#,
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

#[test]
fn the_drawer_shows_the_folders_as_a_tree_in_the_desktops_order() {
    let core = sample();
    let v = core.view();
    let a = &v.accounts[0];
    assert_eq!(a.avatar.initials.chars().count(), 2, "{:?}", a.avatar);
    assert_eq!(a.unread, 3);
    let names: Vec<(&str, usize)> = a.folders.iter().map(|f| (f.path.as_str(), f.depth)).collect();
    assert_eq!(names.first(), Some(&("INBOX", 0)), "the Inbox first: {names:?}");
    let projects = names.iter().position(|(p, _)| *p == "Projects").unwrap();
    assert_eq!(names.get(projects + 1), Some(&("Projects/Short film", 1)), "a folder's folders under it: {names:?}");
    let p = &a.folders[projects];
    assert!(p.parent && !p.collapsed);
}

#[test]
fn accounts_and_folders_collapse_and_it_is_kept() {
    let core = sample();
    act(&core, r#"{"type":"collapse","account":"sample","folder":"Projects"}"#);
    let v = core.view();
    assert!(v.accounts[0].folders.iter().any(|f| f.path == "Projects" && f.collapsed));
    assert!(!v.accounts[0].folders.iter().any(|f| f.path == "Projects/Short film"), "hidden inside its collapsed parent");

    act(&core, r#"{"type":"collapse","account":"sample"}"#);
    let v = core.view();
    assert!(v.accounts[0].collapsed && v.accounts[0].folders.is_empty());
    assert_eq!(v.accounts[0].unread, 3, "the count still shows");

    act(&core, r#"{"type":"collapse","account":"sample"}"#);
    let v = core.view();
    assert!(!v.accounts[0].collapsed);
    assert!(v.accounts[0].folders.iter().any(|f| f.path == "Projects" && f.collapsed), "the folder stays as it was");
}

#[test]
fn the_open_folder_is_marked_and_rows_have_avatars() {
    let core = sample();
    act(&core, r#"{"type":"openFolder","account":"sample","folder":"INBOX"}"#);
    let v = core.view();
    assert!(v.accounts[0].folders.iter().any(|f| f.path == "INBOX" && f.open));
    assert_eq!(v.accounts[0].folders.iter().filter(|f| f.open).count(), 1);
    let zoe = v.messages.iter().find(|m| m["from"].as_str().unwrap_or_default().contains("Zoe")).unwrap();
    assert_eq!(zoe["avatar"]["initials"], "ZP");
}

#[test]
fn a_row_is_archived_alone() {
    let core = sample();
    act(&core, r#"{"type":"openFolder","account":"sample","folder":"INBOX"}"#);
    let keys: Vec<String> = core.view().messages.iter().map(|m| m["key"].as_str().unwrap().to_string()).collect();
    act(&core, &format!(r#"{{"type":"openMessage","key":"{}"}}"#, keys[0]));
    act(&core, &format!(r#"{{"type":"row","key":"{}","action":"archive"}}"#, keys[1]));
    let v = core.view();
    assert_eq!(v.total, 6, "one went");
    assert!(!v.messages.iter().any(|m| m["key"] == keys[1].as_str()));
    assert!(v.messages.iter().any(|m| m["key"] == keys[0].as_str()), "the open one stays");
    let archive = v.accounts[0].folders.iter().find(|f| f.role == "archive").unwrap();
    assert_eq!(archive.total, 2);
}
