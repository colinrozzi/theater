//! Test actor for the `theater:simple/process` handler.
//!
//! Exposes `theater:process-test/probe.*` exports the integration test calls
//! directly, each exercising one path through the real runtime: `run`, a piped
//! spawn/write/read/wait round-trip, a PTY, active-mode delivery into the
//! `process-client` callbacks, and a long-lived child the test uses to check
//! that stopping the actor kills its processes.
//!
//! The record types below mirror `crates/theater-handler-process/process.pact`
//! exactly (field names + order) so the host's interface subset hash matches.

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use packr_guest::{export, import, pack_types, GraphValue, Value, ValueType};
use theater_guest::State;

packr_guest::setup_guest!();

pack_types! {
    record spawn-spec {
        program: string,
        args: list<string>,
        cwd: option<string>,
        env: list<tuple<string, string>>,
        clear-env: bool,
        pty: option<tuple<u16, u16>>,
        stdin: string,
        stdout: string,
        stderr: string,
    }

    record process-status {
        running: bool,
        exit-code: option<s32>,
        signal: option<s32>,
    }

    record output-chunk {
        stream: string,
        data: list<u8>,
    }

    record run-output {
        status: process-status,
        stdout: list<u8>,
        stderr: list<u8>,
        truncated: bool,
    }

    imports {
        theater:simple/self {
            log: func(msg: string),
        }
        theater:simple/process {
            spawn: func(spec: spawn-spec) -> result<string, string>,
            run: func(spec: spawn-spec, input: list<u8>, timeout-ms: option<u64>) -> result<run-output, string>,
            write-stdin: func(process-id: string, data: list<u8>) -> result<u64, string>,
            close-stdin: func(process-id: string) -> result<_, string>,
            read: func(process-id: string, max-bytes: u32, timeout-ms: option<u64>) -> result<option<output-chunk>, string>,
            set-active: func(process-id: string, mode: string) -> result<_, string>,
            wait: func(process-id: string, timeout-ms: option<u64>) -> result<process-status, string>,
            os-pid: func(process-id: string) -> result<u32, string>,
        }
    }
    exports {
        theater:simple/actor.init: func(config: value) -> result<_, string>,
        theater:simple/actor.get-state: func() -> value,
        theater:simple/process-client.on-output: func(process-id: string, stream: string, data: list<u8>) -> result<_, string>,
        theater:simple/process-client.on-exit: func(process-id: string, status: process-status) -> result<_, string>,
        theater:simple/process-client.handle-process-transfer: func(process-id: string) -> result<_, string>,
        theater:process-test/probe.run-echo: func() -> result<string, string>,
        theater:process-test/probe.cat-roundtrip: func(input: string) -> result<string, string>,
        theater:process-test/probe.pty-size: func() -> result<string, string>,
        theater:process-test/probe.start-active: func(script: string) -> result<string, string>,
        theater:process-test/probe.events: func() -> result<string, string>,
        theater:process-test/probe.spawn-sleeper: func() -> result<u32, string>,
    }
}

#[import(module = "theater:simple/self", name = "log")]
fn log(msg: String);

#[import(module = "theater:simple/process", name = "spawn")]
fn process_spawn(spec: Value) -> Value;

#[import(module = "theater:simple/process", name = "run")]
fn process_run(spec: Value, input: Vec<u8>, timeout_ms: Value) -> Value;

#[import(module = "theater:simple/process", name = "write-stdin")]
fn process_write_stdin(process_id: String, data: Vec<u8>) -> Value;

#[import(module = "theater:simple/process", name = "close-stdin")]
fn process_close_stdin(process_id: String) -> Value;

#[import(module = "theater:simple/process", name = "read")]
fn process_read(process_id: String, max_bytes: u32, timeout_ms: Value) -> Value;

#[import(module = "theater:simple/process", name = "set-active")]
fn process_set_active(process_id: String, mode: String) -> Value;

#[import(module = "theater:simple/process", name = "wait")]
fn process_wait(process_id: String, timeout_ms: Value) -> Value;

#[import(module = "theater:simple/process", name = "os-pid")]
fn process_os_pid(process_id: String) -> Value;

/// Callback log: `stdout:"<escaped>"` / `stderr:"<escaped>"` / `exit:<code>` lines.
#[derive(Clone, GraphValue, State)]
struct Events {
    log: String,
}

// ── Value helpers ───────────────────────────────────────────────────────────

fn ok(value: Value) -> Value {
    Value::Result {
        ok_type: value.infer_type(),
        err_type: ValueType::String,
        value: Ok(Box::new(value)),
    }
}

fn ok_unit() -> Value {
    ok(Value::Tuple(vec![]))
}

fn err(msg: String) -> Value {
    Value::Result {
        ok_type: ValueType::String,
        err_type: ValueType::String,
        value: Err(Box::new(Value::String(msg))),
    }
}

/// Unwrap an imported `result<T, string>`.
fn unwrap(v: Value) -> Result<Value, String> {
    match v {
        Value::Result {
            value: Ok(inner), ..
        } => Ok(*inner),
        Value::Result {
            value: Err(inner), ..
        } => match *inner {
            Value::String(e) => Err(e),
            _ => Err(String::from("non-string error")),
        },
        _ => Err(String::from("expected a result")),
    }
}

fn timeout(ms: u64) -> Value {
    Value::Option {
        inner_type: ValueType::U64,
        value: Some(Box::new(Value::U64(ms))),
    }
}

fn field<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    match v {
        Value::Record { fields, .. } => fields.iter().find(|(k, _)| k == name).map(|(_, v)| v),
        _ => None,
    }
}

fn bytes(v: Option<&Value>) -> Vec<u8> {
    match v {
        Some(Value::List { items, .. }) => items
            .iter()
            .filter_map(|i| if let Value::U8(b) = i { Some(*b) } else { None })
            .collect(),
        _ => Vec::new(),
    }
}

fn text(v: Option<&Value>) -> String {
    String::from_utf8_lossy(&bytes(v)).into_owned()
}

fn exit_code(status: &Value) -> Option<i32> {
    match field(status, "exit-code") {
        Some(Value::Option { value: Some(b), .. }) => match **b {
            Value::S32(n) => Some(n),
            _ => None,
        },
        _ => None,
    }
}

fn spawn_spec(program: &str, args: &[&str], pty: Option<(u16, u16)>) -> Value {
    Value::Record {
        type_name: String::from("spawn-spec"),
        fields: vec![
            (
                String::from("program"),
                Value::String(String::from(program)),
            ),
            (
                String::from("args"),
                Value::List {
                    elem_type: ValueType::String,
                    items: args
                        .iter()
                        .map(|a| Value::String(String::from(*a)))
                        .collect(),
                },
            ),
            (
                String::from("cwd"),
                Value::Option {
                    inner_type: ValueType::String,
                    value: None,
                },
            ),
            (
                String::from("env"),
                Value::List {
                    elem_type: ValueType::Tuple(vec![ValueType::String, ValueType::String]),
                    items: Vec::new(),
                },
            ),
            (String::from("clear-env"), Value::Bool(false)),
            (
                String::from("pty"),
                Value::Option {
                    inner_type: ValueType::Tuple(vec![ValueType::U16, ValueType::U16]),
                    value: pty
                        .map(|(c, r)| Box::new(Value::Tuple(vec![Value::U16(c), Value::U16(r)]))),
                },
            ),
            (String::from("stdin"), Value::String(String::new())),
            (String::from("stdout"), Value::String(String::new())),
            (String::from("stderr"), Value::String(String::new())),
        ],
    }
}

fn result_string(r: Result<String, String>) -> Value {
    match r {
        Ok(s) => ok(Value::String(s)),
        Err(e) => err(e),
    }
}

fn record(line: String) {
    Events::with_mut(|e| {
        e.log.push_str(&line);
        e.log.push('\n');
    });
}

// ── lifecycle + process-client callbacks ────────────────────────────────────

#[export(name = "theater:simple/actor.init")]
fn init(_config: Value) -> Value {
    Events::set(Events { log: String::new() });
    log(String::from("process-test: init"));
    ok_unit()
}

#[export(name = "theater:simple/process-client.on-output")]
fn on_output(_process_id: String, stream: String, data: Vec<u8>) -> Value {
    // One line per chunk, payload escaped ({:?}) so multi-line chunks stay on it.
    record(format!("{}:{:?}", stream, String::from_utf8_lossy(&data)));
    ok_unit()
}

#[export(name = "theater:simple/process-client.on-exit")]
fn on_exit(_process_id: String, status: Value) -> Value {
    record(format!("exit:{:?}", exit_code(&status)));
    ok_unit()
}

#[export(name = "theater:simple/process-client.handle-process-transfer")]
fn handle_process_transfer(_process_id: String) -> Value {
    ok_unit()
}

// ── probes ──────────────────────────────────────────────────────────────────

#[export(name = "theater:process-test/probe.run-echo")]
fn run_echo() -> Value {
    result_string((|| {
        let out = unwrap(process_run(
            spawn_spec("sh", &["-c", "echo hello; echo oops >&2; exit 3"], None),
            Vec::new(),
            timeout(10_000),
        ))?;
        let status = field(&out, "status").ok_or("no status")?;
        Ok(format!(
            "stdout={:?} stderr={:?} code={:?}",
            text(field(&out, "stdout")),
            text(field(&out, "stderr")),
            exit_code(status)
        ))
    })())
}

#[export(name = "theater:process-test/probe.cat-roundtrip")]
fn cat_roundtrip(input: String) -> Value {
    result_string((|| {
        let pid = match unwrap(process_spawn(spawn_spec("cat", &[], None)))? {
            Value::String(s) => s,
            _ => return Err(String::from("spawn: expected id")),
        };
        unwrap(process_write_stdin(pid.clone(), input.into_bytes()))?;
        unwrap(process_close_stdin(pid.clone()))?;
        let mut out = Vec::new();
        loop {
            // Small max-bytes so the host's chunk splitting is exercised too.
            match unwrap(process_read(pid.clone(), 3, timeout(5_000)))? {
                Value::Option {
                    value: Some(chunk), ..
                } => out.extend(bytes(field(&chunk, "data"))),
                Value::Option { value: None, .. } => break,
                _ => return Err(String::from("read: expected option")),
            }
        }
        let status = unwrap(process_wait(pid, timeout(5_000)))?;
        Ok(format!(
            "{}|code={:?}",
            String::from_utf8_lossy(&out),
            exit_code(&status)
        ))
    })())
}

#[export(name = "theater:process-test/probe.pty-size")]
fn pty_size() -> Value {
    result_string((|| {
        let out = unwrap(process_run(
            spawn_spec("sh", &["-c", "test -t 1 && stty size"], Some((100, 40))),
            Vec::new(),
            timeout(10_000),
        ))?;
        Ok(String::from(text(field(&out, "stdout")).trim()))
    })())
}

#[export(name = "theater:process-test/probe.start-active")]
fn start_active(script: String) -> Value {
    result_string((|| {
        let pid = match unwrap(process_spawn(spawn_spec("sh", &["-c", &script], None)))? {
            Value::String(s) => s,
            _ => return Err(String::from("spawn: expected id")),
        };
        unwrap(process_set_active(pid.clone(), String::from("active")))?;
        Ok(pid)
    })())
}

#[export(name = "theater:process-test/probe.events")]
fn events() -> Value {
    ok(Value::String(Events::with(|e| e.log.clone())))
}

#[export(name = "theater:process-test/probe.spawn-sleeper")]
fn spawn_sleeper() -> Value {
    let r = (|| {
        let pid = match unwrap(process_spawn(spawn_spec("sleep", &["30"], None)))? {
            Value::String(s) => s,
            _ => return Err(String::from("spawn: expected id")),
        };
        match unwrap(process_os_pid(pid))? {
            Value::U32(n) => Ok(n),
            _ => Err(String::from("os-pid: expected u32")),
        }
    })();
    match r {
        Ok(n) => ok(Value::U32(n)),
        Err(e) => err(e),
    }
}
