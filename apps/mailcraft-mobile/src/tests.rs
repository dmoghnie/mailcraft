use super::*;
use crate::credentials::ShellCredentials;
use crate::outbox::Outbox;
use mailcraft_engine::model::AccountConfig;
use mailcraft_net::worker::Credentials;
use serde_json::Value;
use std::sync::Arc;

/// Messages until one satisfies `f`, for up to ten seconds.
fn until(f: impl Fn(&Value) -> bool) -> Value {
    let end = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < end {
        let m = poll(200);
        if m.is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(&m).unwrap();
        if f(&v) {
            return v;
        }
    }
    panic!("no such message came");
}

/// The whole round: started on an empty folder, an action in, the view out. One test, since
/// the library holds one app per process.
#[test]
fn starts_takes_actions_and_draws() {
    let dir = std::env::temp_dir().join(format!("mailcraft-mobile-native-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    assert!(send(r#"{"type":"folders"}"#).is_err(), "nothing before start");
    assert!(background().is_err());
    assert!(start(dir.to_str().unwrap(), "not json", 0, "UTC").is_err(), "damaged secrets are said");
    start(dir.to_str().unwrap(), r#"{"a1": "pw"}"#, 3600, "Europe/Paris").unwrap();
    start(dir.to_str().unwrap(), "", 0, "").unwrap();

    let first = until(|v| v["kind"] == "render");
    assert_eq!(first["view"]["ready"], true);
    assert_eq!(first["view"]["screen"], "folders");

    assert!(send("{}").unwrap_err().contains("not an action"));
    send(r#"{"type":"startAdding"}"#).unwrap();
    let v = until(|v| v["kind"] == "render" && v["view"]["screen"] == "addAccount");
    assert_eq!(serde_json::from_str::<Value>(&view()).unwrap(), v["view"]);

    send(r#"{"type":"back"}"#).unwrap();
    until(|v| v["kind"] == "render" && v["view"]["screen"] == "folders");
    zone(0, "UTC").unwrap();

    // The cache as the desktop's: the data files' folder made in the app's folder.
    assert!(dir.join("Data Files").is_dir());
    // Out of sight: sealed, and still working after.
    background().unwrap();
    send(r#"{"type":"startAdding"}"#).unwrap();
    until(|v| v["kind"] == "render" && v["view"]["screen"] == "addAccount");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn kept_passwords_are_read_whole_or_refused() {
    assert!(credentials::parse("").unwrap().is_empty());
    assert_eq!(credentials::parse(r#"{"a1":"pw","a1|smtp":"out"}"#).unwrap().len(), 2);
    assert!(credentials::parse("[]").is_err());
    assert!(credentials::parse(r#"{"a1": 5}"#).is_err());
}

#[test]
fn a_password_kept_or_dropped_is_said_to_the_shell_once() {
    let outbox = Arc::new(Outbox::default());
    let c = ShellCredentials::new([("a1".to_string(), "old".to_string())].into(), outbox.clone());
    let a = AccountConfig { id: "a1".into(), ..Default::default() };
    assert_eq!(c.get(&a).as_deref(), Some("old"));
    c.set(&a, "old").unwrap();
    assert_eq!(outbox.next(Duration::ZERO), None, "unchanged: nothing to keep");
    c.set(&a, "new").unwrap();
    let m: Value = serde_json::from_str(&outbox.next(Duration::ZERO).unwrap()).unwrap();
    assert_eq!((m["kind"].as_str(), m["entry"].as_str(), m["value"].as_str()), (Some("secret"), Some("a1"), Some("new")));
    c.set_smtp(&a, "out").unwrap();
    let m: Value = serde_json::from_str(&outbox.next(Duration::ZERO).unwrap()).unwrap();
    assert_eq!(m["entry"], "a1|smtp");
    c.delete(&a);
    c.delete(&a);
    let m: Value = serde_json::from_str(&outbox.next(Duration::ZERO).unwrap()).unwrap();
    assert_eq!((m["kind"].as_str(), m["entry"].as_str()), (Some("forget"), Some("a1")));
    assert_eq!(outbox.next(Duration::ZERO), None, "dropped once");
    assert_eq!(c.get(&a), None);
}

#[test]
fn only_the_newest_view_waits_and_queued_messages_go_first() {
    let o = Outbox::default();
    o.render("1".into());
    o.render("2".into());
    o.push("secret".into());
    assert_eq!(o.next(Duration::ZERO).as_deref(), Some("secret"));
    assert_eq!(o.next(Duration::ZERO).as_deref(), Some(r#"{"kind":"render","view":2}"#));
    assert_eq!(o.next(Duration::ZERO), None);
    o.render("2".into());
    assert_eq!(o.next(Duration::ZERO), None, "the same view isn't drawn twice");
    assert_eq!(o.view(), "2");
}
