//! Message templates: kept from the message being written (without its signature or a reply's
//! quote), put into a new message and into a reply, renamed and deleted, and kept across a
//! restart.

use mailcraft_mime::OutAttachment;

use super::*;

/// A new message with the account's signature, and `text` written above it.
fn writing(s: &mut Session, subject: &str, text: &str) {
    s.run("mail.new", &json!({})).unwrap();
    let body = format!("{text}{}", s.compose.as_ref().unwrap().body);
    s.run("compose.update", &json!({"subject": subject, "body": body, "to": "ann@studio.example"})).unwrap();
}

/// The sample with a signature that goes into new messages and replies.
fn signed(name: &str) -> Session {
    let mut s = sample_session(name);
    let saved = s.run("account.setSignature", &json!({"account": "sample", "signature": "Riley Moreno\nStudio"})).unwrap();
    let sig = saved["saved"].as_str().unwrap().to_string();
    s.run("account.signatureDefaults", &json!({"account": "sample", "new": sig, "reply": sig})).unwrap();
    s
}

#[test]
fn a_template_keeps_the_text_without_the_signature() {
    let mut s = signed("templates-save");
    writing(&mut s, "Weekly numbers", "The numbers for this week are in.");
    assert!(s.compose.as_ref().unwrap().body.contains("Riley Moreno"), "the signature is in the message");
    s.compose.as_mut().unwrap().attachments.push(OutAttachment {
        name: "week.csv".into(),
        mime: "text/csv".into(),
        data: b"a,b\n1,2\n".to_vec(),
        ..Default::default()
    });
    s.run("template.save", &json!({"name": " Weekly "})).unwrap();
    let t = s.template("weekly").cloned().expect("kept, found in any case");
    assert_eq!((t.name.as_str(), t.subject.as_str(), t.body.as_str()), ("Weekly", "Weekly numbers", "The numbers for this week are in."));
    assert!(t.to.is_empty() && t.files.is_empty(), "who it's to and its files only when asked");
    // Asked: kept too. The same name again replaces it.
    s.run("template.save", &json!({"name": "Weekly", "recipients": true, "files": true})).unwrap();
    let listed = s.run("template.list", &json!({})).unwrap();
    assert_eq!(listed["templates"], json!([{"name": "Weekly", "subject": "Weekly numbers", "recipients": true, "pictures": 0, "files": 1}]));
    let t = s.template("Weekly").unwrap();
    assert!(t.to.contains("ann@studio.example") && t.files.first().is_some_and(|f| f.data == b"a,b\n1,2\n"));
}

#[test]
fn a_template_goes_into_a_new_message_and_into_a_reply() {
    let mut s = signed("templates-insert");
    writing(&mut s, "Weekly numbers", "The numbers for this week are in.");
    s.run("template.save", &json!({"name": "Weekly", "recipients": true})).unwrap();
    s.run("compose.discard", &json!({})).unwrap();
    assert!(s.compose.is_none());
    // A new message from it: its text above the signature, its subject, who it's to.
    s.run("template.insert", &json!({"name": "Weekly", "new": true})).unwrap();
    let c = s.compose.as_ref().expect("a new message");
    assert!(c.body.starts_with("The numbers for this week are in.\n"), "{:?}", c.body);
    assert!(c.body.contains("Riley Moreno"), "the signature stays: {:?}", c.body);
    assert_eq!(c.subject, "Weekly numbers");
    assert!(c.to.contains("ann@studio.example"));
    s.run("compose.discard", &json!({})).unwrap();
    // Into a reply: above the quote, which stays; the reply's subject and recipients kept.
    let first = s.list.first().map(|m| m.key.clone()).unwrap();
    s.run("list.open", &json!({"key": first.to_id()})).unwrap();
    s.run("mail.reply", &json!({})).unwrap();
    let (subject, quoted) = {
        let c = s.compose.as_ref().unwrap();
        (c.subject.clone(), c.body.clone())
    };
    let r = s.run("template.insert", &json!({"name": "Weekly"})).unwrap();
    assert_eq!(r["inserted"], json!(0));
    let c = s.compose.as_ref().unwrap();
    assert!(c.body.starts_with("The numbers for this week are in.\n"), "{:?}", c.body);
    assert!(c.body.ends_with(quoted.as_str()), "the reply as it was, under it");
    assert_eq!(c.subject, subject);
    assert!(c.to.contains("ann@studio.example"), "the template's recipient added: {}", c.to);
    // At the cursor, on a line of its own.
    let at = c.body.chars().count();
    s.run("template.insert", &json!({"name": "Weekly", "at": at})).unwrap();
    assert!(s.compose.as_ref().unwrap().body.ends_with("\nThe numbers for this week are in."));
}

#[test]
fn templates_are_renamed_deleted_and_kept_across_a_restart() {
    let mut s = signed("templates-kept");
    writing(&mut s, "Weekly numbers", "The numbers for this week are in.");
    s.run("template.save", &json!({"name": "Weekly"})).unwrap();
    s.run("template.save", &json!({"name": "Thanks"})).unwrap();
    s.run("template.rename", &json!({"name": "weekly", "to": "Monday numbers"})).unwrap();
    // What can't be done changes nothing.
    for (id, bad) in [
        ("template.rename", json!({"name": "Thanks", "to": "Monday numbers"})),
        ("template.rename", json!({"name": "Nothing", "to": "Other"})),
        ("template.save", json!({"name": "  "})),
        ("template.save", json!({"name": "x".repeat(81)})),
        ("template.delete", json!({"name": "Nothing"})),
    ] {
        assert!(matches!(s.run(id, &bad), Err(CmdError::Params(_))), "{id} {bad}");
    }
    s.run("compose.discard", &json!({})).unwrap();
    assert!(matches!(s.run("template.insert", &json!({"name": "Nothing", "new": true})), Err(CmdError::Params(_))));
    assert!(s.compose.is_none(), "an unknown template opens no message");
    // Back after a restart.
    let again = Session::open(s.store.clone(), mailcraft_model::dates::Clock::new(NOW, 0));
    let names: Vec<&str> = again.templates().iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Monday numbers", "Thanks"]);
    assert_eq!(again.template("Monday numbers").map(|t| t.body.as_str()), Some("The numbers for this week are in."));
    s.run("template.delete", &json!({"name": "Thanks"})).unwrap();
    let again = Session::open(s.store.clone(), mailcraft_model::dates::Clock::new(NOW, 0));
    assert_eq!(again.templates().len(), 1);
}

#[test]
fn a_damaged_templates_file_is_read_safely() {
    let s = sample_session("templates-damaged");
    let store = s.store.clone().unwrap();
    store.save_doc("templates.json", &json!({"not": "a list"})).unwrap();
    let again = Session::open(Some(store.clone()), mailcraft_model::dates::Clock::new(NOW, 0));
    assert!(again.templates().is_empty());
    // A file's bytes that aren't base64: that file is left out, the rest kept.
    let one = json!([{"name": "Kept", "body": "Hello", "files": [{"name": "a.txt", "mime": "text/plain", "data": "@@@"}, {"name": "b.txt", "mime": "text/plain", "data": "aGk="}]}]);
    store.save_doc("templates.json", &one).unwrap();
    let again = Session::open(Some(store), mailcraft_model::dates::Clock::new(NOW, 0));
    let t = again.template("Kept").expect("kept");
    assert_eq!(t.files.iter().map(|f| (f.name.as_str(), f.data.as_slice())).collect::<Vec<_>>(), [("b.txt", &b"hi"[..])]);
}
