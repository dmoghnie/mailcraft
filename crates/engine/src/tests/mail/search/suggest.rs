//! Engine tests: suggestions as a search is typed, and the search each stands for.

use mailcraft_model::dates::{civil_from_days, today};

use crate::session::suggest::{SuggestKind, Suggestion};

use super::*;

fn of(s: &mut Session, typed: &str, kind: SuggestKind) -> Vec<Suggestion> {
    s.search_suggestions(typed).into_iter().filter(|x| x.kind == kind).collect()
}

/// What `query` finds over the folder on view's account: the messages' senders and subjects.
fn found(s: &mut Session, query: &str) -> Vec<(String, String)> {
    s.run("search.run", &json!({"query": query})).unwrap();
    assert!(s.search_problems().is_empty(), "{query}: {:?}", s.search_problems());
    let out = s.list.iter().map(|m| (m.from.email.clone(), m.subject.clone())).collect();
    s.run("search.clear", &json!({})).unwrap();
    out
}

#[test]
fn people_become_from_or_to() {
    let mut s = sample_session("suggest-people");
    s.run("folder.open", &json!({"account": "sample", "folder": "INBOX"})).unwrap();
    let ana = of(&mut s, "ana", SuggestKind::Person);
    let ana = ana.iter().find(|x| x.label.contains("Ana Lima")).unwrap();
    assert_eq!(ana.query, "from:ana@soundlab.example");
    assert_eq!(ana.replaces, 3);
    let got = found(&mut s, &ana.query);
    assert!(!got.is_empty() && got.iter().all(|(from, _)| from == "ana@soundlab.example"), "{got:?}");
    // The last word typed: what's before it stays.
    let later = of(&mut s, "budget ana", SuggestKind::Person);
    assert!(later.iter().any(|x| x.query == "from:ana@soundlab.example" && x.replaces == 3));
    // Over Sent, whom it went to.
    s.run("folder.open", &json!({"account": "sample", "folder": "Sent"})).unwrap();
    let shop = of(&mut s, "print", SuggestKind::Person);
    assert!(shop.iter().any(|x| x.query == "to:orders@printshop.example"), "{shop:?}");
}

#[test]
fn subjects_and_files_become_subject_and_attachment() {
    let mut s = sample_session("suggest-subjects");
    super::index_now(&s, "sample");
    s.run("folder.open", &json!({"account": "sample", "folder": "INBOX"})).unwrap();
    // Each conversation once, its subject as written.
    let subjects = of(&mut s, "storyboard", SuggestKind::Subject);
    assert_eq!(subjects.len(), 1, "{subjects:?}");
    assert_eq!(subjects[0].label, "Storyboard review on Thursday");
    assert_eq!(subjects[0].query, "subject:\"Storyboard review on Thursday\"");
    let got = found(&mut s, &subjects[0].query);
    assert_eq!(got.len(), 2, "the message and the answer to it: {got:?}");
    // All of what's typed, not only its last word.
    let two = of(&mut s, "storyboard rev", SuggestKind::Subject);
    assert_eq!(two.first().map(|x| x.replaces), Some("storyboard rev".len()));
    assert!(of(&mut s, "storyboard zebra", SuggestKind::Subject).is_empty());
    // Files whose names have a word beginning so, and none when none has.
    let files = of(&mut s, "colo", SuggestKind::File);
    assert_eq!(files.iter().map(|x| x.query.as_str()).collect::<Vec<_>>(), ["attachment:colo"]);
    assert!(of(&mut s, "zebra", SuggestKind::File).is_empty());
    let got = found(&mut s, "attachment:colo");
    assert!(got.iter().any(|(_, subject)| subject == "Storyboard review on Thursday"), "{got:?}");
}

#[test]
fn dates_become_received() {
    let mut s = sample_session("suggest-dates");
    let words =
        |s: &mut Session, typed: &str| of(s, typed, SuggestKind::Date).into_iter().map(|x| (x.label, x.query, x.replaces)).collect::<Vec<_>>();
    assert_eq!(words(&mut s, "last w"), [("Last week".to_string(), "received:\"last week\"".to_string(), 6)]);
    assert_eq!(words(&mut s, "yest"), [("Yesterday".to_string(), "received:yesterday".to_string(), 4)]);
    assert_eq!(words(&mut s, "this").len(), 3, "this week, month and year");
    // A month: this year's, or last year's when it's still to come.
    let (year, month, _) = civil_from_days(today(&s.clock));
    let march = if month >= 3 { year } else { year - 1 };
    assert_eq!(words(&mut s, "mar"), [(format!("March {march}"), format!("received:{march}-03"), 3)]);
    let december = if month == 12 { year } else { year - 1 };
    assert_eq!(words(&mut s, "dec")[0].1, format!("received:{december}-12"));
    assert!(words(&mut s, "ma").iter().all(|(label, ..)| !label.starts_with("Ma")), "a month from three letters on");
    assert_eq!(words(&mut s, &year.to_string())[0].1, format!("received:{year}"));
    assert!(words(&mut s, &(year + 1).to_string()).is_empty(), "a year still to come");
    // Each is a search the box understands.
    for typed in ["today", "yest", "this w", "last w", "this m", "last m", "this y", "last y", "mar", "dec"] {
        for x in of(&mut s, typed, SuggestKind::Date) {
            found(&mut s, &x.query);
        }
    }
}

#[test]
fn nothing_is_suggested_for_too_little_or_for_syntax() {
    let mut s = sample_session("suggest-none");
    for typed in ["", "a", "  ", "from:ana", "-ana", "budget a"] {
        assert!(s.search_suggestions(typed).is_empty(), "{typed:?}");
    }
    // Nothing of a word nothing has.
    assert!(s.search_suggestions("zqxw").is_empty());
}
