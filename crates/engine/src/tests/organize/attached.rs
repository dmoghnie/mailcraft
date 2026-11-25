//! Engine tests: the Files view, every file mail brought or took that this computer holds,
//! across accounts.

use mailcraft_mime::{Draft, InlinePicture, OutAttachment};
use mailcraft_model::{EmailAddress, Folder, FolderRole, MessageKey};

use super::*;

/// A message of `from` to `to`, with these files, and a picture its text shows (`logo.png`,
/// by `cid:`), dated `date`, come into the sample's `folder`.
fn with_files(s: &mut Session, folder: &str, from: (&str, &str), to: (&str, &str), files: &[(&str, &str)], date: i64, id: &str) -> MessageKey {
    let key = next_key(s, "sample", folder);
    with_files_at(s, key, from, to, files, date, id)
}

/// [`with_files`], as message `key`.
fn with_files_at(s: &mut Session, key: MessageKey, from: (&str, &str), to: (&str, &str), files: &[(&str, &str)], date: i64, id: &str) -> MessageKey {
    let d = Draft {
        to: vec![EmailAddress::new(to.0, to.1)],
        subject: format!("Files from {}", from.0),
        body: "Here they are.".into(),
        html: Some(r#"<p>Here they are.</p><img src="cid:logo@x">"#.into()),
        inline: vec![InlinePicture { cid: "logo@x".into(), name: "logo.png".into(), mime: "image/png".into(), data: vec![1, 2, 3] }],
        attachments: files
            .iter()
            .map(|(name, mime)| OutAttachment { name: (*name).into(), mime: (*mime).into(), data: name.as_bytes().to_vec(), ..Default::default() })
            .collect(),
        ..Default::default()
    };
    let raw = mailcraft_mime::build(&d, &EmailAddress::new(from.0, from.1), date, id).unwrap();
    let summary = mailcraft_mime::summary_from_full(&raw, key, mailcraft_model::Flags::default(), 0, NOW);
    file_in(s, summary, Some(&raw))
}

/// A second account on a server, with its Inbox.
fn second_account(s: &mut Session) -> String {
    let server = s.run("account.add", &json!({"email": "riley@home.example", "password": "pw"})).unwrap()["account"].as_str().unwrap().to_string();
    let inbox = Folder {
        account: server.clone(),
        path: "INBOX".into(),
        delimiter: Some('/'),
        role: FolderRole::Inbox,
        selectable: true,
        ..Default::default()
    };
    s.apply(Event::Folders { account: server.clone(), folders: vec![inbox] });
    s.take_jobs();
    server
}

const ANA: (&str, &str) = ("Ana Lima", "ana@friends.example");
const JUN: (&str, &str) = ("Jun Mori", "jun@partner.example");
const RILEY: (&str, &str) = ("Riley", "riley@studio.example");

/// The sample with Ana's files in its Inbox (and the same message in its Archive), a PDF sent
/// to Jun, and a scan in a second account's Inbox; the Files view's list read.
fn files_session(name: &str) -> (Session, MessageKey, MessageKey, MessageKey) {
    let mut s = sample_session(name);
    let home = second_account(&mut s);
    let files = [
        ("budget.xlsx", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        ("beach.jpg", "image/jpeg"),
        ("photos.zip", "application/zip"),
    ];
    let ana = with_files(&mut s, "INBOX", ANA, RILEY, &files, NOW - 3000, "ana-1@friends.example");
    with_files(&mut s, "Archive", ANA, RILEY, &files, NOW - 3000, "ana-1@friends.example");
    let sent = with_files(&mut s, "Sent", RILEY, JUN, &[("plan.pdf", "application/pdf")], NOW - 2000, "sent-1@studio.example");
    let home_key = MessageKey { account: home, folder: "INBOX".into(), validity: 9, uid: 1 };
    let scan = with_files_at(
        &mut s,
        home_key,
        ANA,
        ("Riley", "riley@home.example"),
        &[("scan.pdf", "application/pdf")],
        NOW - 1000,
        "scan-1@friends.example",
    );
    s.run("view.files", &json!({})).unwrap();
    (s, ana, sent, scan)
}

/// The files of these people, as `files.list` gives them: name, kind and whether sent.
fn listed(s: &mut Session, params: Value) -> Vec<(String, String, bool)> {
    let out = s.run("files.list", &params).unwrap();
    let people = [ANA.1, JUN.1];
    out["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| people.contains(&f["person"].as_str().unwrap()))
        .map(|f| (f["name"].as_str().unwrap().to_string(), f["kind"].as_str().unwrap().to_string(), f["sent"].as_bool().unwrap()))
        .collect()
}

/// Every account's files, newest first, a message in two folders once, the picture the text
/// shows left out; for mail sent, the person is who it went to. Each filter keeps its files.
#[test]
fn files_are_listed_from_every_account_and_filtered() {
    let (mut s, ..) = files_session("files-list");
    let n = |name: &str, kind: &str, sent: bool| (name.to_string(), kind.to_string(), sent);
    let all = listed(&mut s, json!({}));
    assert_eq!(
        all,
        [
            n("scan.pdf", "pdf", false),
            n("plan.pdf", "pdf", true),
            n("budget.xlsx", "document", false),
            n("beach.jpg", "picture", false),
            n("photos.zip", "archive", false)
        ],
        "newest first; the copy in Archive and logo.png not listed"
    );
    assert_eq!(listed(&mut s, json!({"kind": "picture"})), [n("beach.jpg", "picture", false)]);
    assert_eq!(listed(&mut s, json!({"kind": "document"})), [n("budget.xlsx", "document", false)]);
    assert_eq!(listed(&mut s, json!({"kind": "archive"})), [n("photos.zip", "archive", false)]);
    assert_eq!(listed(&mut s, json!({"kind": "pdf"})).len(), 2);
    assert_eq!(listed(&mut s, json!({"person": "JUN"})), [n("plan.pdf", "pdf", true)], "sent to Jun, by any case of the name");
    assert_eq!(listed(&mut s, json!({"person": "friends.example", "name": "BUDG"})), [n("budget.xlsx", "document", false)]);
    assert_eq!(listed(&mut s, json!({"limit": 1})).len(), 1);
    // The view's own filter, as its panes set it.
    s.run("files.filter", &json!({"kind": "pdf", "person": "ana"})).unwrap();
    assert_eq!(s.files_shown().iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["scan.pdf"]);
    s.run("files.filter", &json!({"kind": "all", "person": ""})).unwrap();
    assert!(s.files_shown().len() >= 5);
    assert!(matches!(s.run("files.list", &json!({"kind": "movie"})), Err(CmdError::Params(_))));
    assert!(matches!(s.run("files.filter", &json!({"kind": "movie"})), Err(CmdError::Params(_))));
}

/// The view reads a step at a time: a folder, or a few messages, and is done when every
/// folder and message is read. Opened again, it starts over (mail came meanwhile).
#[test]
fn files_are_read_a_step_at_a_time_and_again_when_opened() {
    let (mut s, ana, ..) = files_session("files-steps");
    assert!(!s.files_done() && s.files_shown().is_empty(), "nothing read before the first step");
    let mut steps = 0;
    while s.files_step() {
        steps += 1;
    }
    assert!(steps >= 4 && s.files_done(), "a folder a step: {steps}");
    let shown = s.files_shown().len();
    // A new file in Ana's message's folder is in the list once the view is opened again.
    with_files(&mut s, "INBOX", ANA, RILEY, &[("later.pdf", "application/pdf")], NOW + 60, "ana-2@friends.example");
    s.run("view.mail", &json!({})).unwrap();
    s.run("view.module", &json!({"name": "files"})).unwrap();
    while s.files_step() {}
    assert_eq!(s.files_shown().len(), shown + 1);
    assert_eq!(s.files_shown().first().map(|f| f.name.as_str()), Some("later.pdf"));
    assert!(s.files_shown().iter().any(|f| f.key == ana));
}

/// A file's preview shows its message in the reading pane, not marked read; it saves the
/// file's own bytes; Go to the message opens it in its folder, in Mail. A file the view didn't
/// find is refused.
#[test]
fn a_file_is_previewed_saved_and_its_message_gone_to() {
    let (mut s, _, sent, scan) = files_session("files-actions");
    // The scan's folder on view behind the Files view: shown, it isn't marked read either.
    s.run("folder.open", &json!({"account": scan.account, "folder": "INBOX"})).unwrap();
    s.run("view.files", &json!({})).unwrap();
    s.run("files.list", &json!({})).unwrap();
    let scan_file = s.files_shown().iter().find(|f| f.key == scan).map(|f| (f.index, f.name.clone())).unwrap();
    let shown = s.run("files.show", &json!({"key": scan.to_id(), "index": scan_file.0})).unwrap();
    assert_eq!(shown["name"], "scan.pdf");
    assert_eq!(s.reading.as_ref().map(|r| r.key.clone()), Some(scan.clone()));
    assert_eq!(s.module, crate::Module::Files, "the preview is beside the files");
    assert_eq!(s.summary(&scan).map(|m| m.flags.seen), Some(false), "not marked read");
    let path = std::env::temp_dir().join(format!("mailcraft-files-{}.pdf", std::process::id()));
    s.run("files.save", &json!({"key": scan.to_id(), "index": scan_file.0, "path": path.to_string_lossy()})).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"scan.pdf");
    let _ = std::fs::remove_file(&path);
    let plan = s.files_shown().iter().find(|f| f.key == sent).map(|f| f.index).unwrap();
    s.run("files.goTo", &json!({"key": sent.to_id(), "index": plan})).unwrap();
    assert_eq!(s.module, crate::Module::Mail);
    assert_eq!(s.reading.as_ref().map(|r| r.key.clone()), Some(sent.clone()));
    assert!(s.list.iter().any(|m| m.key == sent), "its folder is open");
    for bad in [json!({}), json!({"key": scan.to_id()}), json!({"key": scan.to_id(), "index": 9}), json!({"key": "sample:1:999:INBOX", "index": 0})] {
        for id in ["files.show", "files.goTo", "files.save"] {
            assert!(matches!(s.run(id, &bad), Err(CmdError::Params(_))), "{id} {bad}");
        }
    }
}

/// Big files are read a few a step (a step is a frame): two 3 MB files take two steps, where
/// small ones are read twenty a step.
#[test]
fn big_files_are_read_a_few_a_step() {
    let mut s = sample_session("files-big");
    let big = |name: &str| OutAttachment { name: name.into(), mime: "application/pdf".into(), data: vec![b'x'; 3 << 20], ..Default::default() };
    for (i, name) in ["first.pdf", "second.pdf"].into_iter().enumerate() {
        let d = Draft {
            to: vec![EmailAddress::new(RILEY.0, RILEY.1)],
            subject: name.into(),
            body: "Big.".into(),
            attachments: vec![big(name)],
            ..Default::default()
        };
        let raw = mailcraft_mime::build(&d, &EmailAddress::new(JUN.0, JUN.1), NOW + 60 + i as i64, &format!("big-{i}@partner.example")).unwrap();
        let key = next_key(&s, "sample", "INBOX");
        let summary = mailcraft_mime::summary_from_full(&raw, key, mailcraft_model::Flags::default(), 0, NOW);
        file_in(&mut s, summary, Some(&raw));
    }
    s.run("view.files", &json!({})).unwrap();
    let jun = |s: &Session| s.files_shown().iter().filter(|f| f.person.email == JUN.1).count();
    // The folders first, then a step a file.
    while jun(&s) == 0 {
        assert!(s.files_step(), "more to read");
    }
    assert_eq!(jun(&s), 1, "one big file a step");
    s.files_step();
    assert_eq!(jun(&s), 2);
}
