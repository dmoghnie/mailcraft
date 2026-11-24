//! A script's runs on their own: replayed against what its calls were answered, bounded, and
//! never let out of the box they run in.

use proptest::prelude::*;
use serde_json::{Value, json};

use super::run::{Answer, CALLS_MOST, Call, OPS_ALL, PRINTED_MOST, Ran, Runner};
use super::values::{to_json, to_script};

const TOOLS: [&str; 2] = ["find_messages", "read_message"];

fn runner(source: &str) -> Runner {
    Runner::new(source, &TOOLS, 1_791_979_200).unwrap()
}

fn run(source: &str, log: &[(Call, Answer)]) -> Ran {
    runner(source).run(log, OPS_ALL).0
}

fn call(name: &str, input: Value) -> Call {
    Call { name: name.into(), input }
}

fn failed(ran: Ran) -> String {
    match ran {
        Ran::Failed(why) => why,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_script_runs_to_its_end_with_what_it_printed_and_came_to() {
    let ran = run(r#"print("hello"); let n = now(); #{sum: 1 + 2, now: n}"#, &[]);
    assert_eq!(ran, Ran::Done { printed: "hello\n".into(), value: json!({"sum": 3, "now": 1_791_979_200}) });
}

#[test]
fn a_call_not_answered_stops_the_run_and_its_answer_goes_on_from_the_start() {
    let source = r#"print("once"); let f = find_messages(#{query: "invoice"}); let m = read_message(f.messages[0].id); m.text"#;
    let first = call("find_messages", json!({"query": "invoice"}));
    assert_eq!(run(source, &[]), Ran::Needs(first.clone()));
    let found = Answer::Gave(json!({"messages": [{"id": "m0123456789"}], "total": 1}));
    let mut log = vec![(first, found)];
    let read = call("read_message", json!({"id": "m0123456789"}));
    assert_eq!(run(source, &log), Ran::Needs(read.clone()));
    log.push((read, Answer::Gave(json!({"text": "Due Friday"}))));
    // Printed once: only the run that ends is heard.
    assert_eq!(run(source, &log), Ran::Done { printed: "once\n".into(), value: json!("Due Friday") });
}

#[test]
fn a_call_that_failed_is_an_error_the_script_can_catch() {
    let log = [(call("find_messages", json!({})), Answer::Failed("no account's mail may go to this model".into()))];
    let ran = run(r#"try { find_messages() } catch (e) { print(`caught: ${e}`); 0 }"#, &log);
    let Ran::Done { printed, .. } = ran else { panic!("{ran:?}") };
    assert!(printed.contains("caught: no account's mail may go to this model"), "{printed}");
    assert!(failed(run("find_messages()", &log)).contains("no account's mail"));
}

/// A call waiting for its answer stops the run however the script catches errors, even in a
/// closure a built-in calls (whose error Rhai lets a `try` catch).
#[test]
fn a_call_waiting_stops_the_run_whatever_the_script_catches() {
    let source = r#"let r = []; try { r = [1, 2].map(|n| find_messages(#{n: n})); } catch { r = ["caught"]; } r"#;
    assert_eq!(run(source, &[]), Ran::Needs(call("find_messages", json!({"n": 1}))));
    let source = r#"try { find_messages() } catch { "caught" }"#;
    assert_eq!(run(source, &[]), Ran::Needs(call("find_messages", json!({}))));
}

#[test]
fn a_script_that_calls_otherwise_when_run_again_stops() {
    let log = [(call("find_messages", json!({"query": "a"})), Answer::Gave(json!({})))];
    assert!(failed(run(r#"find_messages(#{query: "b"})"#, &log)).contains("other calls than before"));
}

#[test]
fn a_script_makes_so_many_calls_at_most() {
    let log: Vec<(Call, Answer)> = (0..CALLS_MOST).map(|i| (call("find_messages", json!({"i": i})), Answer::Gave(json!({})))).collect();
    let why = failed(run("for i in 0..1000 { find_messages(#{i: i}); }", &log));
    assert!(why.contains(&format!("at most {CALLS_MOST} calls")), "{why}");
}

#[test]
fn a_script_that_goes_on_and_on_is_stopped() {
    let why = failed(run("let n = 0; loop { n += 1; }", &[]));
    assert!(why.contains("operations"), "{why}");
    // What all its runs took together is bounded too.
    let (ran, _) = runner("let n = 0; for i in 0..1000 { n += i; } n").run(&[], 50);
    assert!(failed(ran).contains("in all"));
}

/// Deep values, deep calls and large ones end in an error, never a crash.
#[test]
fn what_runs_out_of_bounds_fails() {
    for source in [
        "let a = []; for i in 0..100000 { a = [a]; } 1",
        "let m = #{}; for i in 0..100000 { m = #{a: m}; } 1",
        "let s = \"x\"; for i in 0..64 { s += s; } 1",
        "fn f(x) { f(x + 1) } f(0)",
        "let a = []; for i in 0..100000 { a.push(i); } 1",
    ] {
        assert!(matches!(run(source, &[]), Ran::Failed(_)), "{source}");
    }
}

#[test]
fn what_a_script_prints_is_bounded() {
    let Ran::Done { printed, .. } = run(r#"for i in 0..5000 { print("0123456789"); }"#, &[]) else { panic!() };
    assert!(printed.chars().count() < PRINTED_MOST + 100, "{}", printed.len());
    assert!(printed.ends_with("is left out.)\n"));
}

/// No `eval`, clock, modules or undeclared names: the script doesn't compile, and says why.
#[test]
fn what_a_script_may_not_use_is_not_there() {
    assert!(Runner::new(r#"eval("1")"#, &TOOLS, 0).is_err());
    assert!(Runner::new(r#"import "x" as y;"#, &TOOLS, 0).is_err());
    assert!(Runner::new("print(nothing_declared)", &TOOLS, 0).is_err_and(|e| e.contains("nothing_declared")));
    assert!(matches!(run("timestamp()", &[]), Ran::Failed(_)));
}

#[test]
fn what_it_printed_before_it_failed_comes_with_why() {
    let why = failed(run(r#"print("got here"); throw "no more""#, &[]));
    assert!(why.contains("no more") && why.contains("got here"), "{why}");
}

#[test]
fn values_nested_past_the_depth_taken_across_are_left_out() {
    let deep = (0..100).fold(json!(1), |v, _| json!([v]));
    let back = to_json(&to_script(&deep));
    let mut depth = 0;
    let mut v = &back;
    while let Some(inner) = v.as_array().and_then(|a| a.first()) {
        depth += 1;
        v = inner;
    }
    assert!(depth < 40, "{depth}");
}

fn json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        (-1e12f64..1e12).prop_filter("not whole", |f| f.fract() != 0.0).prop_map(|f| json!(f)),
        ".{0,12}".prop_map(Value::String),
    ];
    leaf.prop_recursive(4, 32, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::btree_map("[a-z]{1,6}", inner, 0..6).prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

proptest! {
    /// What a tool gives comes back to JSON as it was, through a script's values.
    #[test]
    fn json_round_trips_through_a_scripts_values(v in json_value()) {
        prop_assert_eq!(to_json(&to_script(&v)), v);
    }

    /// A script run again against the same answers comes to the same end.
    #[test]
    fn a_script_run_again_comes_to_the_same_end(v in json_value()) {
        let log = [(call("find_messages", json!({})), Answer::Gave(v))];
        let source = "let f = find_messages(); print(`${f}`); f";
        prop_assert_eq!(run(source, &log), run(source, &log));
    }
}
