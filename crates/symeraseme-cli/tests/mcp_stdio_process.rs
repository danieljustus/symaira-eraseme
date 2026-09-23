//! The actual stdio process must answer before EOF and keep stdout protocol-only.

use base64::{Engine, engine::general_purpose::STANDARD};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[test]
fn stdio_answers_each_request_before_eof_without_stdout_pollution() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/mcp-contract/mcp-stream/cases.json"
    ))
    .unwrap();
    let expected = fixture["cases"][0]["response"].as_str().unwrap();
    let expected: Vec<_> = expected.lines().collect();
    assert_eq!(expected.len(), 2);

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("symeraseme-stdio-{}-{nonce}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let home = root.join("home");
    fs::create_dir(&home).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"))
        .args(["mcp", "--stdio"])
        .current_dir(&root)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            sender.send(line.unwrap()).unwrap();
        }
    });

    input
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n")
        .unwrap();
    input.flush().unwrap();
    let first = receiver.recv_timeout(Duration::from_secs(5));
    if first.is_err() {
        child.kill().ok();
        panic!("stdio did not answer the first request before EOF");
    }
    input.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"initialize\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"initialize\"}\n").unwrap();
    input.flush().unwrap();
    let second = receiver.recv_timeout(Duration::from_secs(5));
    if second.is_err() {
        child.kill().ok();
        panic!("stdio did not answer the second request before EOF");
    }
    drop(input);
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().ok();
            child.wait().ok();
            panic!("stdio did not exit after EOF");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    reader.join().unwrap();
    let stderr = std::io::read_to_string(child.stderr.take().unwrap()).unwrap();
    fs::remove_dir_all(&root).unwrap();

    assert!(status.success(), "stdio exited with {status}: {stderr}");
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");
    assert_eq!(first.unwrap(), expected[0]);
    assert_eq!(second.unwrap(), expected[1]);
    assert!(
        receiver.try_recv().is_err(),
        "extra stdout after two responses"
    );
}

#[test]
fn malformed_stdio_process_matches_go_errors() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../rust-tests/parity/oracle/mcp-stdio-errors/cases.json"
    ))
    .unwrap();
    assert_eq!(fixture["source_revision"], "4e582f28");
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 10);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"))
            .args(["mcp", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(case["input"].as_str().unwrap().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(
            output.status.code(),
            case["exit_code"].as_i64().map(|n| n as i32),
            "{name}: exit"
        );
        assert_eq!(
            output.stdout,
            case["stdout"].as_str().unwrap().as_bytes(),
            "{name}: stdout"
        );
        assert_eq!(
            output.stderr,
            case["stderr"].as_str().unwrap().as_bytes(),
            "{name}: stderr"
        );
    }
}

#[test]
fn stdio_process_matches_go_initialize_id_and_params_corpus() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/mcp-contract/initialize_cases.json"
    ))
    .unwrap();
    assert_eq!(
        fixture["source_revision"],
        "a51c7f3c65218924ce1d505ad8389b2216f08c92"
    );
    assert_eq!(
        fixture["source_path"],
        "internal/mcp/server.go:180-207,377-389"
    );
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 78);
    let parse_errors = cases
        .iter()
        .filter(|case| case["parse_error"].as_bool().unwrap_or(false))
        .count();
    assert_eq!(parse_errors, 6, "pinned Go parse-error corpus size");
    assert_eq!(
        cases.len() - parse_errors,
        72,
        "pinned Go request corpus size"
    );

    let mut input = Vec::new();
    let mut expected = Vec::new();
    let mut request_count = 0;
    for case in cases {
        if case["parse_error"].as_bool().unwrap_or(false) {
            continue;
        }
        let request = case["request"]
            .as_str()
            .map(|value| value.as_bytes().to_vec())
            .or_else(|| {
                case["request_b64"]
                    .as_str()
                    .map(|value| STANDARD.decode(value).unwrap())
            })
            .expect("fixture request");
        input.extend_from_slice(&request);
        request_count += 1;
        if let Some(response) = case["response"].as_str() {
            expected.extend_from_slice(response.as_bytes());
        }
    }
    assert_eq!(request_count, 72, "selected Go request corpus size");

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("symeraseme-mcp008-{}-{nonce}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let home = root.join("home");
    fs::create_dir(&home).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"))
        .args(["mcp", "--stdio"])
        .current_dir(&root)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (writer_tx, writer_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut stdin = stdin;
        let _ = writer_tx.send(stdin.write_all(&input));
    });
    let (stdout_tx, stdout_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut stdout = stdout;
        let _ = stdout_tx.send(stdout.read_to_end(&mut bytes).map(|_| bytes));
    });
    let (stderr_tx, stderr_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut stderr = stderr;
        let _ = stderr_tx.send(stderr.read_to_end(&mut bytes).map(|_| bytes));
    });
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().ok();
            child.wait().ok();
            fs::remove_dir_all(&root).ok();
            panic!("stdio did not finish the bounded initialize corpus");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let remaining = || deadline.saturating_duration_since(Instant::now());
    writer_rx
        .recv_timeout(remaining())
        .expect("bounded stdin write did not finish")
        .unwrap();
    let stdout = stdout_rx
        .recv_timeout(remaining())
        .expect("bounded stdout drain did not finish")
        .unwrap();
    let stderr = stderr_rx
        .recv_timeout(remaining())
        .expect("bounded stderr drain did not finish")
        .unwrap();
    fs::remove_dir_all(&root).unwrap();

    assert!(
        status.success(),
        "stdio exited with {}: {}",
        status,
        String::from_utf8_lossy(&stderr)
    );
    assert_eq!(stdout, expected, "response bytes diverged from Go corpus");
    assert!(
        stderr.is_empty(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&stderr)
    );
}

#[test]
fn malformed_stdio_exits_before_stdin_eof() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"))
        .args(["mcp", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(b"nope").unwrap();
    input.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().ok();
            child.wait().ok();
            panic!("malformed stdio waited for EOF");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(status.code(), Some(1));
    assert_eq!(
        std::io::read_to_string(child.stderr.take().unwrap()).unwrap(),
        "invalid character 'o' in literal null (expecting 'u')\n"
    );
}
