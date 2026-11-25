//! A script's run (Rhai), pure as the engine is: no clock, files, network, modules or `eval`.
//! It runs against a log of what its calls were answered: the first call not answered yet
//! stops the run, for the session to answer (at once, or once a message came or the model
//! answered), and the script runs again from its start with that answer in the log. Each run is
//! bounded (operations, call depth, sizes, calls) and guarded against panics.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rhai::{AST, Dynamic, Engine, EvalAltResult, Map, Position};
use serde_json::{Value, json};

use super::values::{to_json, to_script};

/// Rhai as models get it wrong, writing it as Rust or Python: told with the tool, and again
/// when a script doesn't compile.
pub(crate) const RHAI: &str = "Rhai is neither Rust nor Python: it has no named arguments (find_messages(query: \"x\") doesn't compile), no iter(), collect(), :: or types. An array has map, filter, len, push and contains; for x in array { … } loops; let declares; `${x}` in backticks is a value in a string.";

/// The most calls a script makes, in all.
pub(crate) const CALLS_MOST: usize = 100;
/// The most operations one run takes, and all of a script's runs together.
const OPS_RUN: u64 = 1_000_000;
pub(crate) const OPS_ALL: u64 = 5_000_000;
/// The most a script prints, in characters: past it, the rest is left out.
pub(crate) const PRINTED_MOST: usize = 16_000;
/// Rhai's own bounds: how deep calls, expressions and values go, how large a string, array or
/// map is, how many variables and functions. Each is a bound on the native stack too: a script
/// at all of them at once ends in an error, never an overflow, on a stack of 1 MiB (a Windows
/// program's main thread), a debug build's deeper frames taking half the calls.
const CALL_LEVELS: usize = if cfg!(debug_assertions) { 16 } else { 32 };
const EXPR_DEPTH: usize = 64;
const FN_EXPR_DEPTH: usize = 32;
const DATA_DEPTH: usize = 32;
const STRING_MOST: usize = 1 << 20;
const ARRAY_MOST: usize = 10_000;
const MAP_MOST: usize = 10_000;
const VARIABLES_MOST: usize = 1_000;
const FUNCTIONS_MOST: usize = 200;

/// A call the script made: a tool's name (or `ask`) and what it gave it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Call {
    pub name: String,
    pub input: Value,
}

/// What a call was answered: what it gave, or why it couldn't (an error the script can catch).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Answer {
    Gave(Value),
    Failed(String),
}

/// How a run went.
#[derive(Debug, PartialEq)]
pub(crate) enum Ran {
    /// To its end: what it printed, and what it came to.
    Done { printed: String, value: Value },
    /// Stopped at a call not answered yet: the log's next.
    Needs(Call),
    /// Stopped: why, for the model.
    Failed(String),
}

/// What a run printed, and how many characters that is.
#[derive(Default)]
struct Printed {
    text: String,
    chars: usize,
}

/// The log a run replays, and where it is in it.
#[derive(Default)]
struct Replay {
    log: Vec<(Call, Answer)>,
    at: usize,
    /// The call not answered yet, once the run came to it.
    needs: Option<Call>,
    /// Why the run must stop, when a call did something it may not.
    stopped: Option<String>,
}

impl Replay {
    fn call(&mut self, call: Call) -> Result<Dynamic, Box<EvalAltResult>> {
        if self.needs.is_some() || self.stopped.is_some() {
            return Err(stop());
        }
        match self.log.get(self.at) {
            Some((logged, answer)) if *logged == call => {
                self.at += 1;
                match answer {
                    Answer::Gave(v) => Ok(to_script(v)),
                    Answer::Failed(why) => Err(Box::new(EvalAltResult::ErrorRuntime(why.as_str().into(), Position::NONE))),
                }
            }
            Some(_) => {
                self.stopped = Some("run again, the script made other calls than before".into());
                Err(stop())
            }
            None if self.at >= CALLS_MOST => {
                self.stopped = Some(format!("stopped: a script makes at most {CALLS_MOST} calls"));
                Err(stop())
            }
            None => {
                self.needs = Some(call);
                Err(stop())
            }
        }
    }
}

/// The run stops, as the script can't catch: a call waits for its answer, or may not be made.
/// (Inside a closure a built-in calls, the script could; the replay knows either way.)
fn stop() -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorTerminated(Dynamic::UNIT, Position::NONE))
}

/// A script compiled, with the engine that runs it again and again.
pub(crate) struct Runner {
    engine: Engine,
    ast: AST,
    replay: Rc<RefCell<Replay>>,
    printed: Rc<RefCell<Printed>>,
    ops: Rc<Cell<u64>>,
}

impl Runner {
    /// `source` compiled, its calls made to `tools` (each a function taking a map, or nothing,
    /// as the tool takes its input) and `ask`; `now` is what `now()` gives (Unix seconds). Why
    /// it can't be, for the model.
    pub(crate) fn new(source: &str, tools: &[&str], now: i64) -> Result<Runner, String> {
        let replay = Rc::new(RefCell::new(Replay::default()));
        let printed = Rc::new(RefCell::new(Printed::default()));
        let ops = Rc::new(Cell::new(0));
        let mut engine = Engine::new();
        engine.disable_symbol("eval");
        engine
            .set_max_operations(OPS_RUN)
            .set_max_call_levels(CALL_LEVELS)
            .set_max_expr_depths(EXPR_DEPTH, FN_EXPR_DEPTH)
            .set_max_string_size(STRING_MOST)
            .set_max_array_size(ARRAY_MOST)
            .set_max_map_size(MAP_MOST)
            .set_max_data_depth(DATA_DEPTH)
            .set_max_variables(VARIABLES_MOST)
            .set_max_functions(FUNCTIONS_MOST)
            .set_strict_variables(true);
        let out = printed.clone();
        engine.on_print(move |s| say(&out, s));
        let out = printed.clone();
        engine.on_debug(move |s, _, _| say(&out, s));
        let counted = ops.clone();
        engine.on_progress(move |n| {
            counted.set(n);
            None
        });
        for name in tools.iter().map(|t| (*t).to_string()) {
            let (r, called) = (replay.clone(), name.clone());
            engine.register_fn(&name, move |m: Map| r.borrow_mut().call(Call { name: called.clone(), input: to_json(&Dynamic::from_map(m)) }));
            let (r, called) = (replay.clone(), name.clone());
            engine.register_fn(&name, move || r.borrow_mut().call(Call { name: called.clone(), input: json!({}) }));
        }
        let r = replay.clone();
        engine.register_fn("read_message", move |id: &str| r.borrow_mut().call(Call { name: "read_message".into(), input: json!({"id": id}) }));
        let r = replay.clone();
        engine.register_fn("ask", move |q: &str| r.borrow_mut().call(Call { name: "ask".into(), input: json!({"question": q}) }));
        let r = replay.clone();
        // About any value: a string as it is, `()` (a field a map hasn't got) as none, data as JSON.
        engine.register_fn("ask", move |q: &str, about: Dynamic| {
            let text = match about.clone().into_string() {
                Ok(s) => s,
                Err(_) if about.is_unit() => String::new(),
                Err(_) => to_json(&about).to_string(),
            };
            r.borrow_mut().call(Call { name: "ask".into(), input: json!({"question": q, "text": text}) })
        });
        engine.register_fn("now", move || now);
        let ast = engine.compile(source).map_err(|e| format!("the script doesn't compile: {e}. {RHAI}"))?;
        Ok(Runner { engine, ast, replay, printed, ops })
    }

    /// Run the script from its start against `log`, within `ops_left` operations: how it went,
    /// and the operations it took.
    pub(crate) fn run(&mut self, log: &[(Call, Answer)], ops_left: u64) -> (Ran, u64) {
        *self.replay.borrow_mut() = Replay { log: log.to_vec(), ..Replay::default() };
        *self.printed.borrow_mut() = Printed::default();
        self.ops.set(0);
        self.engine.set_max_operations(ops_left.clamp(1, OPS_RUN));
        let (engine, ast) = (&self.engine, &self.ast);
        let went = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| engine.eval_ast::<Dynamic>(ast)));
        let ops = self.ops.get();
        let replay = std::mem::take(&mut *self.replay.borrow_mut());
        let ran = if let Some(why) = replay.stopped {
            Ran::Failed(why)
        } else if let Some(call) = replay.needs {
            Ran::Needs(call)
        } else {
            match went {
                Ok(Ok(v)) => Ran::Done { printed: std::mem::take(&mut self.printed.borrow_mut().text), value: to_json(&v) },
                Ok(Err(e)) => Ran::Failed(with_printed(failed(&e, ops_left), &self.printed.borrow().text)),
                Err(_) => Ran::Failed("the script stopped unexpectedly".into()),
            }
        };
        (ran, ops)
    }
}

/// Why a run failed, and what it printed before.
fn with_printed(why: String, printed: &str) -> String {
    if printed.trim().is_empty() { why } else { format!("{why}\nWhat it printed before:\n{}", printed.trim_end()) }
}

/// What a run that failed says, for the model.
fn failed(e: &EvalAltResult, ops_left: u64) -> String {
    match e {
        EvalAltResult::ErrorTooManyOperations(_) if ops_left < OPS_RUN => {
            format!("stopped: the script's runs took more than {OPS_ALL} operations in all")
        }
        EvalAltResult::ErrorTooManyOperations(_) => format!("stopped: the script took more than {OPS_RUN} operations"),
        e => format!("the script failed: {e}"),
    }
}

/// `s` printed, on a line of its own, while the output has room; once it's full, a line says
/// the rest was left out.
fn say(out: &RefCell<Printed>, s: &str) {
    let mut out = out.borrow_mut();
    let room = PRINTED_MOST.saturating_sub(out.chars);
    if room == 0 {
        return;
    }
    let line: String = s.chars().take(room).collect();
    let chars = line.chars().count();
    out.text.push_str(&line);
    out.text.push('\n');
    out.chars = out.chars.saturating_add(chars).saturating_add(1);
    if out.chars >= PRINTED_MOST {
        out.text.push_str(&format!("(What it printed past {PRINTED_MOST} characters is left out.)\n"));
    }
}
