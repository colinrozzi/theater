//! Conformance suite for the process handler. Tests the real building blocks
//! (spawn_process, run_process, the output queue, output_pump, terminate, the
//! PTY, the permission checks, the interface) against live child processes.

use super::*;
use std::time::Duration;
use theater::actor::handle::ActorHandle;
use theater::actor::types::ActorOperation;

// ---- helpers ---------------------------------------------------------------

fn spec(program: &str, args: &[&str]) -> SpawnSpec {
    SpawnSpec {
        program: program.to_string(),
        args: args.iter().map(|s| s.to_string()).collect(),
        cwd: None,
        env: Vec::new(),
        clear_env: false,
        pty: None,
        stdin: Disposition::Pipe,
        stdout: Disposition::Pipe,
        stderr: Disposition::Pipe,
    }
}

fn sh(script: &str) -> SpawnSpec {
    spec("sh", &["-c", script])
}

fn pty_sh(script: &str, cols: u16, rows: u16) -> SpawnSpec {
    SpawnSpec {
        pty: Some((cols, rows)),
        ..sh(script)
    }
}

/// Drain every chunk, concatenated per stream: (stdout, stderr).
async fn drain(q: &mut OutputQueue) -> (String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    while let Some((stream, data)) = q.next().await {
        if stream == "stderr" {
            err.extend(data);
        } else {
            out.extend(data);
        }
    }
    (
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    )
}

/// Drain PTY output until `needle` appears (a PTY echoes, so exact output
/// matching is brittle).
async fn read_until(q: &mut OutputQueue, needle: &str) -> String {
    let mut acc = String::new();
    while !acc.contains(needle) {
        match q.next().await {
            Some((_, data)) => acc.push_str(&String::from_utf8_lossy(&data)),
            None => break,
        }
    }
    acc
}

fn pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// What a mock actor saw: ("on-output", stream, data) / ("on-exit", code) …
#[derive(Debug, Clone, PartialEq)]
enum Seen {
    Output(String, Vec<u8>),
    Exit(Option<i32>),
}

/// A mock actor that answers every call and records process-client callbacks.
fn mock_actor() -> (ActorHandle, Arc<std::sync::Mutex<Vec<Seen>>>) {
    let (op_tx, mut op_rx) = tokio::sync::mpsc::channel::<ActorOperation>(4);
    let (info_tx, _info_rx) = tokio::sync::mpsc::channel(4);
    let (ctrl_tx, _ctrl_rx) = tokio::sync::mpsc::channel(4);
    let handle = ActorHandle::new(op_tx, info_tx, ctrl_tx);
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_t = seen.clone();
    tokio::spawn(async move {
        while let Some(ActorOperation::CallFunctionPack {
            name,
            params,
            response_tx,
        }) = op_rx.recv().await
        {
            if let Ok(Value::Tuple(v)) = theater::pack_bridge::decode_value(&params) {
                match name.as_str() {
                    "theater:simple/process-client.on-output" => {
                        let stream = match v.get(1) {
                            Some(Value::String(s)) => s.clone(),
                            _ => String::new(),
                        };
                        let data = bytes_at(&v, 2).unwrap_or_default();
                        seen_t.lock().unwrap().push(Seen::Output(stream, data));
                    }
                    "theater:simple/process-client.on-exit" => {
                        let code = match v.get(1) {
                            Some(Value::Record { fields, .. }) => fields
                                .iter()
                                .find(|(k, _)| k == "exit-code")
                                .and_then(|(_, v)| match v {
                                    Value::Option { value: Some(b), .. } => match **b {
                                        Value::S32(n) => Some(n),
                                        _ => None,
                                    },
                                    _ => None,
                                }),
                            _ => None,
                        };
                        seen_t.lock().unwrap().push(Seen::Exit(code));
                    }
                    _ => {}
                }
            }
            let _ = response_tx.send(Ok(vec![]));
        }
    });
    (handle, seen)
}

async fn poll_until<F: Fn() -> bool>(f: F, within: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + within;
    while tokio::time::Instant::now() < deadline {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    f()
}

fn cfg() -> ProcessHandlerConfig {
    ProcessHandlerConfig::default()
}

// ---- interface -------------------------------------------------------------

#[test]
fn pact_parses_and_names_interface() {
    let iface = process_interface();
    assert_eq!(iface.name(), "theater:simple/process");
    for f in [
        "spawn",
        "run",
        "write-stdin",
        "close-stdin",
        "read",
        "set-active",
        "resize",
        "signal",
        "kill",
        "status",
        "wait",
        "os-pid",
        "list",
        "close",
        "transfer",
    ] {
        assert!(
            iface.hash_subset(&[f]).is_some(),
            "process.pact is missing {f}"
        );
    }
}

#[test]
fn client_pact_parses() {
    let pact = parse_pact(include_str!("../../../pact/process-client.pact"))
        .expect("process-client.pact should be valid");
    assert_eq!(
        InterfaceImpl::from_pact(&pact).name(),
        "theater:simple/process-client"
    );
}

#[test]
fn parse_spawn_spec_record() {
    let v = Value::Record {
        type_name: "spawn-spec".into(),
        fields: vec![
            ("program".into(), Value::String("ls".into())),
            (
                "args".into(),
                Value::List {
                    elem_type: ValueType::String,
                    items: vec![Value::String("-la".into())],
                },
            ),
            (
                "cwd".into(),
                Value::Option {
                    inner_type: ValueType::String,
                    value: Some(Box::new(Value::String("/tmp".into()))),
                },
            ),
            (
                "env".into(),
                Value::List {
                    elem_type: ValueType::Tuple(vec![ValueType::String, ValueType::String]),
                    items: vec![Value::Tuple(vec![
                        Value::String("A".into()),
                        Value::String("1".into()),
                    ])],
                },
            ),
            ("clear-env".into(), Value::Bool(true)),
            (
                "pty".into(),
                Value::Option {
                    inner_type: ValueType::Tuple(vec![ValueType::U16, ValueType::U16]),
                    value: Some(Box::new(Value::Tuple(vec![Value::U16(80), Value::U16(24)]))),
                },
            ),
            ("stdin".into(), Value::String("null".into())),
            ("stdout".into(), Value::String("".into())),
            ("stderr".into(), Value::String("pipe".into())),
        ],
    };
    // Bare record and the single-arg tuple form both parse.
    for input in [v.clone(), Value::Tuple(vec![v])] {
        let s = parse_spawn_spec(&input).unwrap();
        assert_eq!(s.program, "ls");
        assert_eq!(s.args, vec!["-la"]);
        assert_eq!(s.cwd.as_deref(), Some("/tmp"));
        assert_eq!(s.env, vec![("A".to_string(), "1".to_string())]);
        assert!(s.clear_env);
        assert_eq!(s.pty, Some((80, 24)));
        assert_eq!(s.stdin, Disposition::Null);
        assert_eq!(s.stdout, Disposition::Pipe);
    }
}

#[test]
fn parse_spawn_spec_rejects_bad_input() {
    let missing_program = Value::Record {
        type_name: "spawn-spec".into(),
        fields: vec![],
    };
    assert!(parse_spawn_spec(&missing_program).is_err());
    let bad_disposition = Value::Record {
        type_name: "spawn-spec".into(),
        fields: vec![
            ("program".into(), Value::String("ls".into())),
            ("stdout".into(), Value::String("inherit".into())),
        ],
    };
    assert!(parse_spawn_spec(&bad_disposition).is_err());
}

#[test]
fn config_defaults_and_program_policy() {
    let c = ProcessHandlerConfig::default();
    assert!(c.inherit_env);
    assert!(c.check_program("anything").is_ok());
    assert_eq!(c.kill_grace(), Duration::from_millis(DEFAULT_KILL_GRACE_MS));

    let c = ProcessHandlerConfig {
        allowed_programs: Some(vec!["cargo".into(), "git".into()]),
        ..Default::default()
    };
    assert!(c.check_program("cargo").is_ok());
    assert!(c.check_program("bash").is_err());
    // Exact match: a path to an allowed name is a different program string.
    assert!(c.check_program("/usr/bin/cargo").is_err());

    let c = ProcessHandlerConfig {
        allowed_programs: Some(vec!["*".into()]),
        ..Default::default()
    };
    assert!(c.check_program("bash").is_ok());
}

// ---- pipes -----------------------------------------------------------------

#[tokio::test]
async fn stdout_stderr_and_exit_code() {
    let mut sp = spawn_process(&sh("echo out; echo err >&2; exit 3"), &cfg()).unwrap();
    let (out, err) = drain(&mut sp.output).await;
    assert_eq!(out, "out\n");
    assert_eq!(err, "err\n");
    let exit = wait_exit(&mut sp.status).await;
    assert_eq!(
        exit,
        ExitInfo {
            code: Some(3),
            signal: None
        }
    );
}

#[tokio::test]
async fn stdin_roundtrip() {
    let mut sp = spawn_process(&spec("cat", &[]), &cfg()).unwrap();
    let mut stdin = sp.stdin.take().expect("piped stdin");
    stdin.write_all(b"hello theater").await.unwrap();
    drop(stdin); // EOF -> cat exits
    let (out, _) = drain(&mut sp.output).await;
    assert_eq!(out, "hello theater");
    assert_eq!(wait_exit(&mut sp.status).await.code, Some(0));
}

#[tokio::test]
async fn null_dispositions_and_env_and_cwd() {
    let dir = std::env::temp_dir();
    let mut s = sh("echo \"$GREETING\"; pwd; echo hidden >&2");
    s.env = vec![("GREETING".into(), "hi".into())];
    s.cwd = Some(dir.to_string_lossy().into_owned());
    s.stderr = Disposition::Null;
    let mut sp = spawn_process(&s, &cfg()).unwrap();
    let (out, err) = drain(&mut sp.output).await;
    let canon = std::fs::canonicalize(&dir).unwrap();
    assert!(out.starts_with("hi\n"), "{out:?}");
    assert!(
        out.contains(&*dir.to_string_lossy()) || out.contains(&*canon.to_string_lossy()),
        "{out:?}"
    );
    assert_eq!(err, "", "stderr=null must produce no stderr chunks");
}

#[tokio::test]
async fn clear_env_and_inherit_env_false() {
    std::env::set_var("THEATER_PROCESS_TEST_LEAK", "secret");
    let script = "echo \"[${THEATER_PROCESS_TEST_LEAK}]\"";
    let mut sp = spawn_process(&sh(script), &cfg()).unwrap();
    assert_eq!(drain(&mut sp.output).await.0, "[secret]\n");

    // clear-env per spawn (sh is found via the parent's PATH; the child's env is empty).
    let mut s = spec("/bin/sh", &["-c", script]);
    s.clear_env = true;
    let mut sp = spawn_process(&s, &cfg()).unwrap();
    assert_eq!(drain(&mut sp.output).await.0, "[]\n");

    // inherit_env = false in config does the same for every spawn.
    let no_inherit = ProcessHandlerConfig {
        inherit_env: false,
        ..Default::default()
    };
    let mut sp = spawn_process(&spec("/bin/sh", &["-c", script]), &no_inherit).unwrap();
    assert_eq!(drain(&mut sp.output).await.0, "[]\n");
}

#[tokio::test]
async fn spawn_failure_is_an_error() {
    let err = spawn_process(&spec("/definitely/not/a/program", &[]), &cfg())
        .err()
        .expect("spawn should fail");
    assert!(err.contains("spawn"), "{err}");
}

#[tokio::test]
async fn large_output_backpressures_without_loss() {
    // ~8 MiB: far more than the bounded queue holds, so readers must block on
    // a full queue (not drop) while we drain slowly.
    let mut sp = spawn_process(&sh("head -c 8388608 /dev/zero | tr '\\0' 'x'"), &cfg()).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await; // let it fill
    let mut total = 0usize;
    while let Some((_, data)) = sp.output.next().await {
        assert!(data.iter().all(|b| *b == b'x'));
        total += data.len();
    }
    assert_eq!(total, 8 * 1024 * 1024);
}

#[tokio::test]
async fn grandchild_holding_pipe_does_not_hang_output() {
    // The background sleep inherits stdout and outlives sh; output must still
    // end shortly after sh exits (DRAIN_GRACE), not after 30s.
    let start = std::time::Instant::now();
    let mut sp = spawn_process(&sh("sleep 30 & echo done"), &cfg()).unwrap();
    let os_pid = sp.os_pid;
    let (out, _) = tokio::time::timeout(Duration::from_secs(5), drain(&mut sp.output))
        .await
        .expect("output should end after the drain grace");
    assert_eq!(out, "done\n");
    assert!(start.elapsed() < Duration::from_secs(5));
    let _ = signal_group(os_pid, libc::SIGKILL); // reap the sleeper
}

// ---- signals / groups ------------------------------------------------------

#[tokio::test]
async fn signal_group_reaches_descendants() {
    let mut sp = spawn_process(&sh("sleep 30 & echo $!; wait"), &cfg()).unwrap();
    let line = read_until(&mut sp.output, "\n").await;
    let grandchild: u32 = line.trim().parse().expect("grandchild pid");
    assert!(pid_alive(grandchild));

    signal_group(sp.os_pid, libc::SIGTERM).unwrap();
    let exit = tokio::time::timeout(Duration::from_secs(5), wait_exit(&mut sp.status))
        .await
        .expect("sh should die");
    assert_eq!(exit.signal, Some(libc::SIGTERM));
    assert!(
        poll_until(|| !pid_alive(grandchild), Duration::from_secs(5)).await,
        "the backgrounded grandchild should have been signalled too"
    );
}

#[tokio::test]
async fn signal_to_gone_group_is_ok() {
    let mut sp = spawn_process(&sh("true"), &cfg()).unwrap();
    wait_exit(&mut sp.status).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(signal_group(sp.os_pid, libc::SIGKILL).is_ok());
}

// ---- PTY -------------------------------------------------------------------

#[tokio::test]
async fn pty_is_a_tty_with_size_and_term() {
    let mut sp = spawn_process(
        &pty_sh(
            "test -t 0 && test -t 1 && echo IS_TTY; stty size; echo TERM=$TERM",
            100,
            40,
        ),
        &cfg(),
    )
    .unwrap();
    assert!(sp.stdin.is_none() && sp.pty.is_some());
    let out = read_until(&mut sp.output, "TERM=").await;
    let out = out + &drain(&mut sp.output).await.0;
    assert!(out.contains("IS_TTY"), "{out:?}");
    assert!(
        out.contains("40 100"),
        "stty size should be rows cols: {out:?}"
    );
    assert!(out.contains("TERM=xterm-256color"), "{out:?}");
    assert_eq!(wait_exit(&mut sp.status).await.code, Some(0));
}

#[tokio::test]
async fn pty_input_and_resize() {
    let mut sp = spawn_process(
        &pty_sh("read line; echo GOT:$line; read _; stty size", 80, 24),
        &cfg(),
    )
    .unwrap();
    let master = sp.pty.clone().unwrap();
    master.write_all(b"hello\n").await.unwrap();
    let out = read_until(&mut sp.output, "GOT:hello").await;
    assert!(out.contains("GOT:hello"), "{out:?}");

    master.resize(132, 50).unwrap();
    master.write_all(b"\n").await.unwrap();
    let out = read_until(&mut sp.output, "50 132").await;
    assert!(out.contains("50 132"), "{out:?}");
    assert_eq!(wait_exit(&mut sp.status).await.code, Some(0));
}

#[tokio::test]
async fn pty_eof_char_ends_input() {
    let mut sp = spawn_process(&pty_sh("cat; echo CAT_DONE", 80, 24), &cfg()).unwrap();
    let master = sp.pty.clone().unwrap();
    master.write_all(&[PTY_EOF]).await.unwrap();
    let out = read_until(&mut sp.output, "CAT_DONE").await;
    assert!(out.contains("CAT_DONE"), "{out:?}");
    assert_eq!(wait_exit(&mut sp.status).await.code, Some(0));
}

#[tokio::test]
async fn pty_ctrl_c_interrupts_foreground() {
    let mut sp = spawn_process(&pty_sh("echo READY; sleep 30", 80, 24), &cfg()).unwrap();
    read_until(&mut sp.output, "READY").await;
    sp.pty.clone().unwrap().write_all(&[0x03]).await.unwrap();
    let exit = tokio::time::timeout(Duration::from_secs(5), wait_exit(&mut sp.status))
        .await
        .expect("^C through the PTY should interrupt");
    assert_eq!(exit.signal, Some(libc::SIGINT));
}

// ---- run -------------------------------------------------------------------

#[tokio::test]
async fn run_collects_output_and_status() {
    let out = run_process(
        &sh("cat; echo err >&2; exit 7"),
        &cfg(),
        b"piped in".to_vec(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(out.stdout, b"piped in");
    assert_eq!(out.stderr, b"err\n");
    assert_eq!(out.exit.code, Some(7));
    assert!(!out.truncated);
}

#[tokio::test]
async fn run_truncates_at_cap_without_blocking() {
    let small = ProcessHandlerConfig {
        max_run_output_bytes: Some(1000),
        ..Default::default()
    };
    let out = run_process(
        &sh("head -c 1000000 /dev/zero"),
        &small,
        Vec::new(),
        Some(10_000),
    )
    .await
    .unwrap();
    assert!(out.truncated);
    assert!(out.stdout.len() <= 1000);
    assert_eq!(out.exit.code, Some(0));
}

#[tokio::test]
async fn run_timeout_kills_the_group() {
    let start = std::time::Instant::now();
    let err = run_process(&sh("sleep 30 & sleep 30"), &cfg(), Vec::new(), Some(200))
        .await
        .err()
        .expect("should time out");
    assert!(err.starts_with("timeout"), "{err}");
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn run_on_a_pty() {
    let out = run_process(
        &pty_sh("test -t 1 && echo PTY_OK", 80, 24),
        &cfg(),
        Vec::new(),
        Some(10_000),
    )
    .await
    .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("PTY_OK"));
}

// ---- passive split / active delivery order ---------------------------------

#[tokio::test]
async fn pending_remainder_is_returned_first() {
    let mut sp = spawn_process(&sh("printf abcdef"), &cfg()).unwrap();
    // Simulate a short passive read: take one chunk, put back its tail.
    let (stream, mut data) = sp.output.next().await.unwrap();
    let rest = data.split_off(2);
    sp.output.pending = Some((stream, rest));
    assert_eq!(data, b"ab");
    let (out, _) = drain(&mut sp.output).await;
    assert_eq!(out, "cdef");
}

fn entry_from(sp: Spawned, owner: TheaterId) -> ProcessEntry {
    ProcessEntry {
        owner,
        os_pid: sp.os_pid,
        pty: sp.pty,
        stdin: sp.stdin.map(|s| Arc::new(Mutex::new(Some(s)))),
        output: Arc::new(Mutex::new(sp.output)),
        status: sp.status,
        mode: DataMode::Passive,
        pump: None,
        tasks: sp.tasks,
    }
}

#[tokio::test]
async fn active_mode_delivers_all_output_then_exit() {
    let (actor, seen) = mock_actor();
    let st = Arc::new(SharedProcessState::new());
    let sp = spawn_process(&sh("echo one; echo two >&2; echo three; exit 5"), &cfg()).unwrap();
    let mut e = entry_from(sp, TheaterId::generate());
    e.mode = DataMode::Active;
    start_pump(st.clone(), 1, &mut e, actor, false);

    assert!(
        poll_until(
            || seen
                .lock()
                .unwrap()
                .iter()
                .any(|s| matches!(s, Seen::Exit(_))),
            Duration::from_secs(5)
        )
        .await
    );
    let seen = seen.lock().unwrap().clone();
    assert_eq!(
        seen.last(),
        Some(&Seen::Exit(Some(5))),
        "on-exit must be last"
    );
    let (mut out, mut err) = (Vec::<u8>::new(), Vec::<u8>::new());
    for s in &seen[..seen.len() - 1] {
        match s {
            Seen::Output(stream, d) if stream == "stdout" => out.extend(d),
            Seen::Output(stream, d) if stream == "stderr" => err.extend(d),
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(out, b"one\nthree\n");
    assert_eq!(err, b"two\n");
}

#[tokio::test]
async fn once_mode_delivers_one_then_reverts() {
    let (actor, seen) = mock_actor();
    let st = Arc::new(SharedProcessState::new());
    let sp = spawn_process(&sh("echo a; sleep 0.2; echo b"), &cfg()).unwrap();
    let mut e = entry_from(sp, TheaterId::generate());
    e.mode = DataMode::Once;
    start_pump(st.clone(), 1, &mut e, actor, true);
    st.processes.lock().await.insert(1, e);

    assert!(poll_until(|| seen.lock().unwrap().len() == 1, Duration::from_secs(5)).await);
    assert!(
        poll_until(
            || st
                .processes
                .try_lock()
                .map(|p| p[&1].mode == DataMode::Passive && p[&1].pump.is_none())
                .unwrap_or(false),
            Duration::from_secs(5)
        )
        .await,
        "once should revert to passive"
    );
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "no further pushes after once"
    );
    // The rest is still there for a passive reader.
    let output = st.processes.lock().await[&1].output.clone();
    let (out, _) = drain(&mut *output.lock().await).await;
    assert_eq!(out, "b\n");
}

#[tokio::test]
async fn exit_is_delivered_only_once_across_mode_flips() {
    let (actor, seen) = mock_actor();
    let st = Arc::new(SharedProcessState::new());
    let sp = spawn_process(&sh("true"), &cfg()).unwrap();
    let mut e = entry_from(sp, TheaterId::generate());
    start_pump(st.clone(), 1, &mut e, actor.clone(), false);
    assert!(poll_until(|| seen.lock().unwrap().len() == 1, Duration::from_secs(5)).await);
    e.stop_pump();
    start_pump(st.clone(), 1, &mut e, actor, false);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(seen.lock().unwrap().as_slice(), &[Seen::Exit(Some(0))]);
}

// ---- teardown --------------------------------------------------------------

#[tokio::test]
async fn terminate_sigterms_then_sigkills_stragglers() {
    let owner = TheaterId::generate();
    let polite = spawn_process(&sh("sleep 30"), &cfg()).unwrap();
    let stubborn = spawn_process(&sh("trap '' TERM; echo READY; sleep 30 & wait"), &cfg()).unwrap();
    let stubborn = entry_from(stubborn, owner);
    {
        let output = stubborn.output.clone();
        read_until(&mut *output.lock().await, "READY").await;
    }
    let polite = entry_from(polite, owner);
    let (ps, ss) = (polite.status.clone(), stubborn.status.clone());

    let start = std::time::Instant::now();
    terminate(vec![polite, stubborn], Duration::from_millis(300)).await;
    assert!(start.elapsed() < Duration::from_secs(5));
    // Both were reaped (not left as zombies) before terminate returned.
    let polite_exit = (*ps.borrow()).expect("polite process reaped");
    let stubborn_exit = (*ss.borrow()).expect("stubborn process reaped");
    assert_eq!(polite_exit.signal, Some(libc::SIGTERM));
    assert_eq!(
        stubborn_exit.signal,
        Some(libc::SIGKILL),
        "a TERM-ignoring process must be SIGKILLed after the grace"
    );
}
