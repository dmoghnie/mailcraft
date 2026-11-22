//! UI tests: the buttons on new-mail notifications, each reaching its command and the message
//! its notification told of.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;

/// What the system was asked to show: each notice and its buttons' ids.
type Shown = Rc<RefCell<Vec<(u64, Vec<String>)>>>;
/// The buttons the test presses, for the app to take.
type Pressed = Rc<RefCell<Vec<(u64, String)>>>;

/// Services whose notifications the test presses.
struct Pressing {
    shown: Shown,
    pressed: Pressed,
}

impl crate::Services for Pressing {
    stub_services!();
    fn notify_mail(&mut self, _wake: &egui::Context, n: &crate::MailNotice<'_>) {
        assert!(!n.title.is_empty() && !n.more.is_empty());
        self.shown.borrow_mut().push((n.id, n.buttons.iter().map(|(id, _)| (*id).to_string()).collect()));
    }
    fn notices_pressed(&mut self) -> Vec<(u64, String)> {
        std::mem::take(&mut *self.pressed.borrow_mut())
    }
}

/// The app with pressing services, its Inbox shown.
fn pressing(name: &str) -> (MailApp, egui::Context, Shown, Pressed) {
    let (mut a, _) = app(name);
    let (shown, pressed) = (Shown::default(), Pressed::default());
    a.services = Box::new(Pressing { shown: shown.clone(), pressed: pressed.clone() });
    let ctx = egui::Context::default();
    frame(&ctx, &mut a, vec![]);
    (a, ctx, shown, pressed)
}

/// These messages announced: their notification's id and buttons.
fn announce(a: &mut MailApp, ctx: &egui::Context, shown: &Shown, news: &[Summary]) -> (u64, Vec<String>) {
    a.session.new_mail = news.to_vec();
    frame(ctx, a, vec![]);
    shown.borrow().last().cloned().expect("a notification")
}

fn inbox(a: &MailApp) -> Vec<Summary> {
    a.session.list.iter().map(|m| (**m).clone()).collect()
}

fn open(a: &mut MailApp, folder: &str) {
    a.run("folder.open", json!({"account": "sample", "folder": folder}));
}

#[test]
fn each_button_reaches_its_command_and_message() {
    let (mut a, ctx, shown, pressed) = pressing("notice-buttons");
    let mail = inbox(&a);
    assert!(mail.len() >= 4, "the sample's Inbox has mail");
    let unread = mail.iter().find(|m| !m.flags.seen).cloned().expect("an unread message");
    let others: Vec<Summary> = mail.iter().filter(|m| m.key != unread.key).take(3).cloned().collect();
    let (archived, deleted, answered) = (&others[0], &others[1], &others[2]);

    // One message: four buttons. Mark as read marks that one read, the folder on view left be.
    let (notice, buttons) = announce(&mut a, &ctx, &shown, std::slice::from_ref(&unread));
    assert_eq!(buttons, ["read", "archive", "delete", "reply"]);
    open(&mut a, "Sent");
    let sent = a.session.view.clone();
    pressed.borrow_mut().push((notice, "read".into()));
    frame(&ctx, &mut a, vec![]);
    assert_eq!(a.session.view, sent, "the folder on view stays");
    open(&mut a, "INBOX");
    assert!(a.session.summary(&unread.key).is_some_and(|m| m.flags.seen), "read");

    // Archive and Delete, each its own notification's message.
    let (archive, _) = announce(&mut a, &ctx, &shown, std::slice::from_ref(archived));
    let (delete, _) = announce(&mut a, &ctx, &shown, std::slice::from_ref(deleted));
    pressed.borrow_mut().extend([(delete, "delete".to_string()), (archive, "archive".to_string())]);
    frame(&ctx, &mut a, vec![]);
    assert!(a.session.summary(&archived.key).is_none() && a.session.summary(&deleted.key).is_none());
    open(&mut a, "Archive");
    assert!(a.session.list.iter().any(|m| m.message_id == archived.message_id), "archived");
    open(&mut a, "Trash");
    assert!(a.session.list.iter().any(|m| m.message_id == deleted.message_id), "deleted");

    // Reply: the message opened in its folder, its answer being written.
    a.run("view.module", json!({"name": "calendar"}));
    let (reply, _) = announce(&mut a, &ctx, &shown, std::slice::from_ref(answered));
    pressed.borrow_mut().push((reply, "reply".into()));
    frame(&ctx, &mut a, vec![]);
    assert_eq!(a.session.module, mailcraft_engine::Module::Mail);
    assert_eq!(a.session.reading.as_ref().map(|r| r.key.clone()), Some(answered.key.clone()));
    let subject = a.session.compose.as_ref().map(|c| c.subject.clone()).expect("a reply being written");
    assert!(subject.ends_with(&answered.subject), "{subject}");
}

#[test]
fn several_messages_offer_mark_all_as_read_alone() {
    let (mut a, ctx, shown, pressed) = pressing("notice-several");
    let unread: Vec<Summary> = inbox(&a).into_iter().filter(|m| !m.flags.seen).take(2).collect();
    assert_eq!(unread.len(), 2, "the sample has unread mail");
    let (notice, buttons) = announce(&mut a, &ctx, &shown, &unread);
    assert_eq!(buttons, ["readAll"]);
    let read = |a: &MailApp| unread.iter().filter(|m| a.session.summary(&m.key).is_some_and(|x| x.flags.seen)).count();
    // A button it hasn't, and a notification never shown, do nothing.
    pressed.borrow_mut().extend([(notice, "archive".to_string()), (notice + 99, "readAll".to_string())]);
    frame(&ctx, &mut a, vec![]);
    assert_eq!(read(&a), 0);
    pressed.borrow_mut().push((notice, "readAll".into()));
    frame(&ctx, &mut a, vec![]);
    assert_eq!(read(&a), 2, "every one read");
}

/// Reply from a notification waits for its message to come, and is given up when another
/// is opened meanwhile.
#[test]
fn a_notifications_reply_waits_for_its_message() {
    let (mut a, ctx, _, _) = pressing("notice-reply-waits");
    let mail = inbox(&a);
    let (first, second) = (&mail[0], &mail[1]);
    a.run("list.open", json!({"key": first.key.to_id()}));
    let body = a.session.reading.as_mut().and_then(|r| r.parsed.take());
    assert!(body.is_some());
    a.ui.reply_due = Some(first.key.clone());
    frame(&ctx, &mut a, vec![]);
    assert!(a.session.compose.is_none(), "not before it's come");
    if let Some(r) = a.session.reading.as_mut() {
        r.parsed = body;
    }
    frame(&ctx, &mut a, vec![]);
    assert!(a.session.compose.is_some(), "written once it has");
    assert!(a.ui.reply_due.is_none());

    a.run("compose.discard", json!({}));
    a.ui.reply_due = Some(first.key.clone());
    a.run("list.open", json!({"key": second.key.to_id()}));
    frame(&ctx, &mut a, vec![]);
    assert!(a.ui.reply_due.is_none() && a.session.compose.is_none(), "given up for another");
}
