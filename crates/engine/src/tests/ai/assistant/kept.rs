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

/// Read again after MailCraft closed while it waited (for the model's answer, or a message a
/// read needs), a chat says the answer never came, and the record no longer says it waits;
/// sent again, it goes on.
#[test]
fn a_chat_cut_short_by_closing_says_so_when_read_again() {
    let mut s = ready("assistant-cut-short");
    let c = send(&mut s, None, "Anything new?");
    question(&mut s);
    let store = s.store.clone().unwrap();
    let open = || Session::open(Some(store.clone()), mailcraft_model::dates::Clock::new(NOW, 0));
    let mut again = open();
    let v = chat(&mut again, &c);
    assert_eq!(v["chat"]["error"], "MailCraft closed before the answer came: send again to go on", "{v}");
    let log = again.run("ai.log", &json!({})).unwrap();
    assert!(log.to_string().contains("\"failed\"") && !log.to_string().contains("\"waiting\""), "{log}");
    again.run("ai.allow", &json!({"account": "sample", "reach": "local"})).unwrap();
    local(&mut again, "Local");
    send(&mut again, Some(&c), "Again?");
    let (q, asked) = question(&mut again);
    assert!(asked.turns.first().is_some_and(|t| t.text.ends_with(")\n\nAnything new?")), "{:?}", asked.turns.first().map(|t| &t.text));
    answer(&mut again, q, "Nothing.");
    again.take_jobs();
    assert!(chat(&mut open(), &c)["chat"]["error"].is_null(), "answered: nothing cut short");
    // A read that waited for its message as MailCraft closed: failed, and the chat says so.
    let file = format!("assistant-{c}.json");
    let kept: crate::Chat = store.load_doc(&file).unwrap();
    let call = AiToolCall { id: "call_0".into(), name: "read_message".into(), input: json!({"id": "m1"}) };
    let turn = crate::ChatTurn {
        turn: mailcraft_model::AiTurn { role: AiRole::Assistant, text: String::new(), calls: vec![call], results: Vec::new() },
        thinking: String::new(),
        context: Vec::new(),
        context_text: String::new(),
        cut: false,
        at: NOW,
    };
    let run = crate::ToolRun {
        call: "call_0".into(),
        tool: "read_message".into(),
        state: RunState::Waiting,
        what: "Read a message".into(),
        messages: Vec::new(),
        found: Vec::new(),
        result: String::new(),
        part_of: None,
        input: Value::Null,
    };
    store.save_doc(&file, &crate::Chat { turns: [kept.turns.clone(), vec![turn]].concat(), runs: vec![run], ..kept }).unwrap();
    let v = chat(&mut open(), &c);
    assert_eq!(v["chat"]["runs"][0]["state"], "failed", "{v}");
    assert_eq!(v["chat"]["runs"][0]["result"], "MailCraft closed before the message came");
    assert_eq!(v["chat"]["error"], "MailCraft closed before the answer came: send again to go on");
}

/// Deleted, a chat's copy set aside as damaged goes with it; Delete all leaves no chat file
/// at all, listed or not, nor the list's damaged copy.
#[test]
fn deleting_chats_leaves_no_copy_of_them() {
    let mut s = ready("assistant-no-copies");
    let c = send(&mut s, None, "Keep this");
    let (q, _) = question(&mut s);
    answer(&mut s, q, "Kept.");
    s.take_jobs();
    let store = s.store.clone().unwrap();
    let root = store.root().to_path_buf();
    std::fs::write(root.join(format!("assistant-{c}.json.bad")), b"{ mail").unwrap();
    s.run("assistant.delete", &json!({"chat": c})).unwrap();
    assert!(!root.join(format!("assistant-{c}.json.bad")).exists(), "its damaged copy deleted with it");
    let d = send(&mut s, None, "And this");
    let (q, _) = question(&mut s);
    answer(&mut s, q, "Kept.");
    s.take_jobs();
    for left in ["assistant-c90.json.bad", "assistant-c91.json", "assistant.json.bad"] {
        std::fs::write(root.join(left), b"{ mail").unwrap();
    }
    assert_eq!(s.run("assistant.deleteAll", &json!({})).unwrap()["deleted"], 1);
    s.take_jobs();
    let files: Vec<String> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(files.iter().all(|f| !f.starts_with("assistant") || f == "assistant.json"), "{d}: {files:?}");
}
