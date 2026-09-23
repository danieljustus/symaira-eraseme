//! The actual stdio process must answer before EOF and keep stdout protocol-only.

use std::fs;
use std::io::{BufRead, BufReader, Write};
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
