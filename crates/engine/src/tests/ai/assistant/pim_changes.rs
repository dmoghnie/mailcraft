//! The changes MailCraft's AI proposes to the calendar, the contacts, the tasks and the notes:
//! each said on its card and made only once the person approves it, of what the chat found and
//! its model may read; a meeting with others left to the calendar, where they're told; nothing
//! sent.

use super::*;
use mailcraft_model::ical;

/// The model calls `tools`, which run at once (or are refused): what each gave, and whether it
/// failed; and the question it's asked next.
fn results(s: &mut Session, q: u64, tools: &[(&str, Value)]) -> (u64, Vec<(String, bool)>) {
    calls(s, q, "", tools);
    let (q, asked) = question(s);
    (q, asked.turns.last().unwrap().results.iter().map(|r| (r.content.clone(), r.error)).collect())
}

/// What the change proposed last says on its card: it waits for the person.
fn proposed(s: &mut Session, c: &str) -> String {
    let v = chat(s, c);
    let run = v["chat"]["runs"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(run["state"], "proposed", "{run}");
    run["what"].as_str().unwrap().to_string()
}

/// The person approves what waits: nothing's sent; what the model is told of it, and the
/// question that tells it.
fn approve(s: &mut Session, c: &str) -> (u64, String) {
    s.run("assistant.decide", &json!({"chat": c, "approve": true})).unwrap();
    let jobs = s.take_jobs();
    assert!(!jobs.iter().any(|j| matches!(j, Job::Send { .. })), "nothing sent");
    let (q, asked) = jobs
        .into_iter()
        .find_map(|j| match j {
            Job::Ai { work: AiWork::Ask { id, chat, .. } } => Some((id, chat)),
            _ => None,
        })
        .expect("asked again");
    (q, asked.turns.last().unwrap().results[0].content.clone())
}

/// The name a listing gives the row with `text`.
fn named(listing: &str, text: &str) -> String {
    bracketed(listing.lines().find(|l| l.contains(text)).unwrap_or_else(|| panic!("no {text} in {listing}")))
}

/// The name a result says something is now: its last word.
fn last_word(done: &str) -> String {
    done.trim_end_matches(['.', ' ']).trim_end_matches(" now").rsplit(' ').next().unwrap().to_string()
}

/// Someone is added, changed and deleted by the name the chat gave them, each once approved;
/// someone whose address is there already isn't added again.
#[test]
fn contacts_are_added_changed_and_deleted_once_approved() {
    let mut s = ready("assistant-people");
    let mina = |s: &Session| s.people("mina@gallery.example").first().map(|p| p.card.clone());
    let c = send(&mut s, None, "Add Mina Cho from the gallery");
    let (q, asked) = question(&mut s);
    let names: Vec<&str> = asked.tools.iter().map(|t| t.name.as_str()).collect();
    for t in [
        "find_tasks",
        "find_notes",
        "change_event",
        "add_person",
        "change_person",
        "add_task",
        "change_task",
        "add_note",
        "change_note",
        "delete_items",
    ] {
        assert!(names.contains(&t), "{t}: {names:?}");
    }
    assert!(asked.system.contains("t0123456789") && asked.system.contains("n0123456789"), "tasks and notes are named too");
    let input = json!({"name": "Mina Cho", "emails": "mina@gallery.example", "phones": "+1 555 0123", "organization": "Community Gallery", "title": "Curator"});
    calls(&mut s, q, "", &[("add_person", input)]);
    assert_eq!(
        proposed(&mut s, &c),
        "Add Mina Cho to the contacts: email mina@gallery.example; phone +1 555 0123; organization Community Gallery; title Curator"
    );
    assert!(mina(&s).is_none(), "nothing added before it's approved");
    let (q, done) = approve(&mut s, &c);
    assert!(done.starts_with("Approved. Done: added to the contacts, as p"), "{done}");
    let card = mina(&s).expect("added");
    assert_eq!((card.title.as_str(), card.org.as_str()), ("Curator", "Community Gallery"));
    let person = last_word(&done);
    // What's said changes; the rest stays.
    calls(&mut s, q, "", &[("change_person", json!({"id": person, "title": "Head curator"}))]);
    assert_eq!(proposed(&mut s, &c), "Change Mina Cho in the contacts: their title to Head curator");
    let (q, done) = approve(&mut s, &c);
    assert_eq!(done, "Approved. Done: changed.");
    let card = mina(&s).unwrap();
    assert_eq!((card.title.as_str(), card.phones.len()), ("Head curator", 1));
    let (q, r) = results(&mut s, q, &[("add_person", json!({"name": "Zoe P", "emails": "ZOE@studio.example"}))]);
    assert!(r[0].1 && r[0].0.contains("Zoe Park has ZOE@studio.example already"), "{r:?}");
    calls(&mut s, q, "", &[("delete_items", json!({"ids": [person]}))]);
    assert_eq!(proposed(&mut s, &c), "Delete the contact Mina Cho");
    let (q, done) = approve(&mut s, &c);
    assert_eq!(done, "Approved. Done: 1 deleted.");
    assert!(mina(&s).is_none());
    answer(&mut s, q, "Done.");
    let e = s.run("assistant.open", &json!({"chat": c, "link": format!("mailcraft:{person}")})).unwrap_err();
    assert!(e.to_string().contains("isn't in the address book any more"), "{e}");
}

/// An event is changed and deleted by the name the chat found it by, once approved: one moved is
/// named anew. Not the time of one that repeats (it would move every time), and of a meeting
/// with others only what's the person's own, its reminder: they'd have to be told.
#[test]
fn events_are_changed_and_deleted_once_approved_but_not_a_meeting_with_others() {
    let mut s = ready("assistant-events-changed");
    let event = |s: &Session, uid: &str| s.find_event(&format!("{uid}@sample.mailcraft")).map(|(_, e)| e.event.clone());
    let tomorrow = mailcraft_model::dates::day_string(mailcraft_model::dates::today(&s.clock) + 1);
    let c = send(&mut s, None, "Tidy my calendar");
    let (q, _) = question(&mut s);
    let finds =
        [("find_events", json!({"query": "lunch"})), ("find_events", json!({"query": "team"})), ("find_events", json!({"query": "storyboard"}))];
    let (q, found) = results(&mut s, q, &finds);
    let (lunch, standup, review) = (bracketed(&found[0].0), bracketed(&found[1].0), bracketed(&found[2].0));
    calls(&mut s, q, "", &[("change_event", json!({"id": lunch, "location": "Noodle bar", "reminder": 30}))]);
    let what = proposed(&mut s, &c);
    assert!(
        what.starts_with("Change “Lunch with Jun”, ") && what.ends_with(", in Calendar: its place to Noodle bar; a reminder 30 minutes before"),
        "{what}"
    );
    assert_eq!(event(&s, "lunch").unwrap().location, "Café on the corner", "nothing changed before it's approved");
    let (q, done) = approve(&mut s, &c);
    assert_eq!(done, format!("Approved. Done: changed; it's {lunch} now."));
    let e = event(&s, "lunch").unwrap();
    assert_eq!((e.location.as_str(), ical::reminder_minutes(&e)), ("Noodle bar", Some(30)));
    // Moved, it keeps its length, and is named anew.
    calls(&mut s, q, "", &[("change_event", json!({"id": lunch, "start": format!("{tomorrow}T13:00")}))]);
    assert!(proposed(&mut s, &c).ends_with("13:00 to 14:00"));
    let (q, done) = approve(&mut s, &c);
    let moved = last_word(&done);
    assert!(moved.starts_with('e') && moved != lunch, "{done}");
    let (q, r) = results(
        &mut s,
        q,
        &[
            ("change_event", json!({"id": standup, "start": format!("{tomorrow}T10:00")})),
            ("change_event", json!({"id": review, "summary": "Storyboard review, moved"})),
            ("delete_items", json!({"ids": [review]})),
            ("change_event", json!({"id": moved, "location": "Noodle bar"})),
            ("change_event", json!({"id": moved})),
        ],
    );
    assert!(r.iter().all(|(_, error)| *error), "{r:?}");
    for (n, why) in [
        "“Team stand-up” repeats: its time is changed in the calendar",
        "“Storyboard review” is a meeting with others: they'd have to be told of the change",
        "“Storyboard review” is a meeting with others: they'd have to be told it's off",
        "nothing would change",
        "say what changes",
    ]
    .iter()
    .enumerate()
    {
        assert!(r[n].0.contains(why), "{n}: {}", r[n].0);
    }
    // A meeting's reminder is the person's own: changed, and the meeting's version stays.
    let version = event(&s, "review").unwrap().sequence;
    calls(&mut s, q, "", &[("change_event", json!({"id": review, "reminder": 60}))]);
    let (q, _) = approve(&mut s, &c);
    let e = event(&s, "review").unwrap();
    assert_eq!((ical::reminder_minutes(&e), e.sequence), (Some(60), version));
    // Of one that repeats, the time named alone; then, asked, every time.
    calls(&mut s, q, "", &[("delete_items", json!({"ids": [standup]}))]);
    assert!(proposed(&mut s, &c).ends_with("(that time only: it repeats)"));
    let (q, done) = approve(&mut s, &c);
    assert_eq!(done, "Approved. Done: 1 deleted.");
    assert_eq!(event(&s, "standup").map(|e| e.exdates.len()), Some(1), "the series stays, less that time");
    calls(&mut s, q, "", &[("delete_items", json!({"ids": [standup], "series": true}))]);
    assert_eq!(proposed(&mut s, &c), "Delete the event “Team stand-up”, every time it repeats");
    approve(&mut s, &c);
    assert!(event(&s, "standup").is_none());
}

/// Tasks and notes are found as the window lists them, and added, changed and deleted by the
/// names the chat gave them once approved; a task that repeats, marked done, is due again. An
/// answer's links to them open them: Tasks, Notes.
#[test]
fn tasks_and_notes_are_found_added_changed_and_deleted_once_approved() {
    let mut s = ready("assistant-todo");
    let today = mailcraft_model::dates::today(&s.clock);
    let task = |s: &Session, title: &str| s.tasks(title).first().map(|t| t.task.clone());
    let note = |s: &Session, text: &str| s.notes(text).first().map(|n| n.note.text.clone());
    let c = send(&mut s, None, "What's on my list?");
    let (q, _) = question(&mut s);
    let (q, found) = results(&mut s, q, &[("find_tasks", json!({})), ("find_notes", json!({"query": "poster"}))]);
    let (tasks, notes) = (&found[0].0, &found[1].0);
    assert!(tasks.contains("Send the grading notes to the client | Due: "), "{tasks}");
    assert!(tasks.contains("| in progress, 50% | high priority | Categories: Work | Notes: Reels 2 and 3; the café scene is warmer now."), "{tasks}");
    assert!(tasks.lines().any(|l| l.contains("Order the festival posters | Due: ") && l.contains("(late) | not started")), "{tasks}");
    assert!(tasks.lines().any(|l| l.contains("Back up the edit drives") && l.contains("| repeats")), "{tasks}");
    assert!(notes.contains("| Festival poster / Approve the proof by Thursday; ask about 170 gsm paper.") && notes.ends_with("1 found."), "{notes}");
    let (backup, poster) = (named(tasks, "Back up the edit drives"), bracketed(notes));
    assert!(backup.starts_with('t') && poster.starts_with('n'), "{backup} {poster}");
    let due = mailcraft_model::dates::day_string(today + 1);
    calls(&mut s, q, "", &[("add_task", json!({"title": "Call the print shop", "due": due, "priority": "high"}))]);
    let day = mailcraft_model::dates::day_text(today + 1, &s.clock);
    assert_eq!(proposed(&mut s, &c), format!("Add the task “Call the print shop”: due {day}; high priority"));
    assert!(task(&s, "print shop").is_none(), "nothing added before it's approved");
    let (q, done) = approve(&mut s, &c);
    assert!(done.starts_with("Approved. Done: added to the tasks, as t"), "{done}");
    let added = last_word(&done);
    let t = task(&s, "print shop").unwrap();
    assert_eq!((t.due_day(), t.priority.code()), (Some(today + 1), "high"));
    // A task that repeats, marked done, is due again its next time.
    let was = task(&s, "edit drives").unwrap().due_day().unwrap();
    calls(&mut s, q, "", &[("change_task", json!({"id": backup, "done": true}))]);
    assert_eq!(proposed(&mut s, &c), "Change the task “Back up the edit drives”: done");
    let (q, done) = approve(&mut s, &c);
    assert!(done.starts_with("Approved. Done: marked done; it repeats, so it's due again "), "{done}");
    let t = task(&s, "edit drives").unwrap();
    assert_eq!((t.is_done(), t.due_day()), (false, Some(was + 7)));
    calls(&mut s, q, "", &[("add_note", json!({"text": "Print shop\nAsk for the matte proofs."}))]);
    assert_eq!(proposed(&mut s, &c), "Add the note “Print shop”");
    let (q, done) = approve(&mut s, &c);
    let new_note = last_word(&done);
    assert!(done.starts_with("Approved. Done: added to the notes, as n"), "{done}");
    calls(&mut s, q, "", &[("change_note", json!({"id": new_note, "text": "Print shop\nAsk for the matte proofs by Friday."}))]);
    assert_eq!(proposed(&mut s, &c), "Change the note “Print shop” to “Print shop Ask for the matte proofs by Friday.”");
    let (q, _) = approve(&mut s, &c);
    assert_eq!(note(&s, "matte").as_deref(), Some("Print shop\nAsk for the matte proofs by Friday."));
    calls(&mut s, q, "", &[("delete_items", json!({"ids": [added, new_note]}))]);
    assert_eq!(proposed(&mut s, &c), "Delete the task “Call the print shop”; the note “Print shop”");
    let (q, done) = approve(&mut s, &c);
    assert_eq!(done, "Approved. Done: 2 deleted.");
    assert!(task(&s, "print shop").is_none() && note(&s, "matte").is_none());
    answer(&mut s, q, &format!("[Back up](mailcraft:{backup}), [the poster](mailcraft:{poster})."));
    let opened = s.run("assistant.open", &json!({"chat": c, "link": format!("mailcraft:{backup}")})).unwrap();
    assert_eq!((opened["task"].as_str(), s.module), (Some("task-backup@sample.mailcraft"), crate::Module::Tasks), "{opened}");
    let opened = s.run("assistant.open", &json!({"chat": c, "link": format!("mailcraft:{poster}")})).unwrap();
    assert_eq!((opened["note"].as_str(), s.module), (Some("note-poster@sample.mailcraft"), crate::Module::Notes), "{opened}");
}

/// What's changed or deleted is only what the chat found, of the kind the tool is for, and
/// checked as the window's forms check it; each refused goes back to the model at once. A model
/// the account's mail may not go to finds no tasks or notes, and adds nothing.
#[test]
fn pim_changes_are_checked_as_they_are_proposed() {
    let mut s = ready("assistant-pim-checked");
    send(&mut s, None, "Change things");
    let (q, _) = question(&mut s);
    let (q, found) = results(&mut s, q, &[("find_people", json!({"query": "zoe"})), ("find_messages", json!({"query": STORYBOARD}))]);
    let (zoe, message) = (bracketed(&found[0].0), bracketed(&found[1].0));
    let tries = [
        ("change_person", json!({"title": "Producer"}), "`id` is required"),
        ("change_person", json!({"id": "p0123456789", "title": "Producer"}), "isn't an id the tools gave"),
        ("change_event", json!({"id": zoe, "summary": "Lunch"}), "is a person, not an event"),
        ("change_person", json!({"id": zoe}), "say what changes"),
        ("change_person", json!({"id": zoe, "emails": "not an address"}), "is not an email address"),
        ("delete_items", json!({"ids": [message]}), "is a message: delete_messages deletes messages"),
        ("delete_items", json!({"ids": []}), "name between 1 and 50"),
        ("add_task", json!({"due": "2026-11-24"}), "`title` is required"),
        ("add_task", json!({"title": "Call", "due": "soon"}), "is not a date"),
        ("add_note", json!({"text": "  "}), "`text` is required"),
    ];
    let (_, r) = results(&mut s, q, &tries.iter().map(|(t, input, _)| (*t, input.clone())).collect::<Vec<_>>());
    for (n, (_, _, why)) in tries.iter().enumerate() {
        assert!(r[n].1 && r[n].0.contains(why), "{n}: {}", r[n].0);
    }
    let cloud = cloud(&mut s, "Claude");
    s.run("ai.use", &json!({"task": "chat", "profile": cloud})).unwrap();
    s.take_jobs();
    send(&mut s, None, "What's on my list?");
    let (q, _) = question(&mut s);
    let tries = [
        ("find_tasks", json!({})),
        ("find_notes", json!({})),
        ("add_task", json!({"title": "Call"})),
        ("add_note", json!({"text": "Call"})),
        ("add_person", json!({"name": "Mina Cho"})),
    ];
    let (_, r) = results(&mut s, q, &tries);
    assert_eq!((r[0].0.as_str(), r[1].0.as_str()), ("No tasks found.", "No notes found."));
    assert!(r[2..].iter().all(|(said, error)| *error && said.contains("mail may go to Claude")), "{r:?}");
}

/// An event or a note two accounts hold under one uid (a meeting invited at two of the person's
/// addresses) is changed and deleted in the account the chat found it in; the other's copy is
/// left as it was.
#[test]
fn what_two_accounts_hold_under_one_uid_is_changed_in_the_one_found() {
    let mut s = ready("assistant-one-uid");
    let home = s.run("account.add", &json!({"email": "riley@home.example", "password": "pw"})).unwrap()["account"].as_str().unwrap().to_string();
    s.run("ai.allow", &json!({"account": home, "reach": "local"})).unwrap();
    let (lunch, poster) = ("lunch@sample.mailcraft", "note-poster@sample.mailcraft");
    let event = s.find_event(lunch).map(|(_, e)| e.event.clone()).unwrap();
    s.create_event(Some(&home), None, ical::CalEvent { summary: "Lunch at home".into(), ..event }).unwrap();
    let note = s.notes("poster").first().map(|n| n.note.clone()).unwrap();
    let copy = mailcraft_model::pim::Note { text: "Home poster\nThe draft.".into(), ..note };
    s.pim_mut(&home).notes.push(mailcraft_model::pim::StoredNote { note: copy, ..Default::default() });
    s.take_jobs();
    let place = |s: &Session, account: &str| {
        s.pim.get(account).and_then(|p| p.events.iter().find(|e| e.event.uid == lunch)).map(|e| e.event.location.clone())
    };
    let text =
        |s: &Session, account: &str| s.pim.get(account).and_then(|p| p.notes.iter().find(|n| n.note.uid == poster)).map(|n| n.note.text.clone());
    let c = send(&mut s, None, "Tidy my calendar and notes");
    let (q, _) = question(&mut s);
    let (q, found) = results(&mut s, q, &[("find_events", json!({"query": "lunch"})), ("find_notes", json!({"query": "poster"}))]);
    let (events, notes) = (&found[0].0, &found[1].0);
    let (at_work, at_home) = (named(events, "Lunch with Jun"), named(events, "Lunch at home"));
    let (work_note, home_note) = (named(notes, "Festival poster"), named(notes, "Home poster"));
    calls(
        &mut s,
        q,
        "",
        &[
            ("change_event", json!({"id": at_home, "location": "Kitchen"})),
            ("change_note", json!({"id": home_note, "text": "Home poster\nThe final."})),
        ],
    );
    let (q, _) = approve(&mut s, &c);
    assert_eq!(place(&s, &home).as_deref(), Some("Kitchen"));
    assert_eq!(place(&s, "sample").as_deref(), Some("Café on the corner"), "the other copy as it was");
    assert_eq!(text(&s, &home).as_deref(), Some("Home poster\nThe final."));
    assert!(text(&s, "sample").is_some_and(|t| t.starts_with("Festival poster")));
    calls(&mut s, q, "", &[("delete_items", json!({"ids": [at_work, work_note]}))]);
    approve(&mut s, &c);
    assert_eq!((place(&s, "sample"), text(&s, "sample")), (None, None), "deleted in the account found");
    assert_eq!(place(&s, &home).as_deref(), Some("Kitchen"), "the other copy kept");
    assert!(text(&s, &home).is_some());
}
