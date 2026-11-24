//! The chats kept on this computer: read again after a restart, deleted whole.

use super::*;

/// Chats are kept on this computer, each in its own file, read again after a restart; one
/// deleted is gone from it.
#[test]
fn chats_are_kept_on_this_computer_and_deleted_whole() {
    let mut s = ready("assistant-kept");
    let c = send(&mut s, None, "Remember this chat\nplease");
    let (q, _) = question(&mut s);
    answer(&mut s, q, "Remembered.");
    s.take_jobs();
    let store = s.store.clone().unwrap();
    let file = store.root().join(format!("assistant-{c}.json"));
    assert!(file.exists());
    let mut again = Session::open(Some(store.clone()), mailcraft_model::dates::Clock::new(NOW, 0));
    let list = again.run("assistant.chats", &json!({})).unwrap();
    assert_eq!(list["chats"][0]["title"], "Remember this chat");
    let v = chat(&mut again, &c);
    assert_eq!(v["chat"]["turns"][1]["text"], "Remembered.");
    let log = again.run("ai.log", &json!({})).unwrap().to_string();
    assert!(!log.contains("Remember"), "the record holds no chat: {log}");
    again.run("assistant.delete", &json!({"chat": c})).unwrap();
    again.take_jobs();
    assert!(!file.exists());
    let again = Session::open(Some(store), mailcraft_model::dates::Clock::new(NOW, 0)).run("assistant.chats", &json!({})).unwrap();
    assert_eq!(again["chats"], json!([]));
}

/// A damaged list of chats is set aside and made again from the chats' files: none goes missing
/// from it (nor from Delete all), and a new chat never takes the id of one kept.
#[test]
fn a_damaged_list_of_chats_is_made_again_from_their_files() {
    let mut s = ready("assistant-list-damaged");
    let kept: Vec<String> = ["First chat", "Second chat"]
        .iter()
        .map(|said| {
            let c = send(&mut s, None, said);
            let (q, _) = question(&mut s);
            answer(&mut s, q, "Noted.");
            c
        })
        .collect();
    s.take_jobs();
    let store = s.store.clone().unwrap();
    std::fs::write(store.root().join("assistant.json"), b"{ damaged").unwrap();
    let mut again = Session::open(Some(store.clone()), mailcraft_model::dates::Clock::new(NOW, 0));
    let list = again.run("assistant.chats", &json!({})).unwrap();
    let mut ids: Vec<String> = list["chats"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect();
    ids.sort();
    assert_eq!(ids, kept, "{list}");
    again.run("ai.allow", &json!({"account": "sample", "reach": "local"})).unwrap();
    local(&mut again, "Local");
    let new = send(&mut again, None, "Third chat");
    assert!(!kept.contains(&new), "{new}");
    again.take_jobs();
    let v = chat(&mut again, &kept[0]);
    assert_eq!(v["chat"]["turns"][1]["text"], "Noted.", "not written over");
    // Read again, the list holds all three.
    let list = Session::open(Some(store), mailcraft_model::dates::Clock::new(NOW, 0)).run("assistant.chats", &json!({})).unwrap();
    assert_eq!(list["chats"].as_array().unwrap().len(), 3, "{list}");
}
