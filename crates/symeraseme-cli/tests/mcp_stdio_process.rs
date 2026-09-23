//! The actual stdio process must answer before EOF and keep stdout protocol-only.

use base64::{Engine, engine::general_purpose::STANDARD};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

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
#[cfg(unix)]
fn scheduler_tools_match_source_bound_go_with_private_crontab() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/mcp-contract/mcp-003-scheduler/cases.json"
    ))
    .expect("scheduler oracle fixture");
    assert_eq!(
        fixture["source_revision"],
        "4af87d9d2cd127722aa4d0e3942057b0365bd7d9"
    );
    assert_eq!(
        fixture["source_files"],
        serde_json::json!([
            {
                "path": "internal/mcp/contract_handler.go",
                "sha256": "b70d4a121aaced8a0efc548380e95f2618c5a172f456c0e944a36921338b488f"
            },
            {
                "path": "internal/scheduler/scheduler.go",
                "sha256": "46b18551267d75eeeeb675f1f6af00e3201c63e3db64307a174ccc5f327c3138"
            }
        ])
    );
    let source_bytes = [
        include_bytes!("../../../internal/mcp/contract_handler.go").as_slice(),
        include_bytes!("../../../internal/scheduler/scheduler.go").as_slice(),
    ];
    for (source, bytes) in fixture["source_files"]
        .as_array()
        .expect("source file metadata")
        .iter()
        .zip(source_bytes)
    {
        let actual_hash: String = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(
            actual_hash,
            source["sha256"].as_str().expect("source file hash"),
            "Go oracle source drift: {}",
            source["path"].as_str().expect("source file path")
        );
    }
    let cases = fixture["cases"].as_array().expect("cases");
    assert_eq!(cases.len(), 4);

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "symeraseme-mcp-scheduler-{}-{nonce}",
        std::process::id()
    ));
    let home = root.join("home");
    let data = root.join("data");
    let tmp = root.join("tmp");
    let bin = root.join("bin");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(&tmp).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let canonical_root = fs::canonicalize(&root).unwrap();
    let state = root.join("crontab");
    fs::write(&state, "# user schedule\n").unwrap();
    let fake_crontab = format!(
        "#!/bin/sh\ncase \"$1\" in\n  -l)\n    [ -f \"$SCHEDULER_CRONTAB_STATE\" ] || exit 1\n    while IFS= read -r line; do printf '%s\\n' \"$line\"; done < \"$SCHEDULER_CRONTAB_STATE\"\n    ;;\n  *)\n    while IFS= read -r line; do printf '%s\\n' \"$line\"; done < \"$1\" > \"$SCHEDULER_CRONTAB_STATE\"\n    ;;\nesac\n"
    );
    fs::write(bin.join("crontab"), fake_crontab).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(bin.join("crontab"), fs::Permissions::from_mode(0o700)).unwrap();
    }

    let mut child = Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"))
        .args(["mcp", "--stdio"])
        .current_dir(&root)
        .env_clear()
        .env("PATH", &bin)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("TMPDIR", &tmp)
        .env("SYMERASEME_DATA_DIR", &data)
        .env("SCHEDULER_CRONTAB_STATE", &state)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut input = child.stdin.take().unwrap();
        for case in cases {
            input
                .write_all(case["request"].as_str().unwrap().as_bytes())
                .unwrap();
            input.write_all(b"\n").unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout)
        .unwrap()
        .replace(canonical_root.to_string_lossy().as_ref(), "<SCHEDULE_ROOT>")
        .replace(root.to_string_lossy().as_ref(), "<SCHEDULE_ROOT>");
    let expected = cases
        .iter()
        .map(|case| case["response"].as_str().unwrap())
        .collect::<Vec<_>>()
        .concat();
    assert_eq!(actual, expected);
    assert_eq!(fixture["side_effects"]["crontab_after_install"], true);

    let mut actual_files: Vec<_> = fs::read_dir(root.join("schedules"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    actual_files.sort();
    let expected_files: Vec<_> = fixture["side_effects"]["files_after_install"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(actual_files, expected_files);
    assert_eq!(
        fs::read_to_string(state).unwrap(),
        fixture["side_effects"]["crontab_after_uninstall"]
    );
    fs::remove_dir_all(root).unwrap();

    let native_cases = fixture["native_install_cases"].as_array().unwrap();
    assert_eq!(native_cases.len(), 2);
    for case in native_cases {
        run_native_scheduler_case(case);
    }
}

#[cfg(unix)]
fn run_native_scheduler_case(case: &serde_json::Value) {
    let request: serde_json::Value =
        serde_json::from_str(case["request"].as_str().unwrap()).unwrap();
    let platform = request["params"]["arguments"]["platform"].as_str().unwrap();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "symeraseme-mcp-scheduler-{platform}-{}-{nonce}",
        std::process::id()
    ));
    let home = root.join("home");
    let data = root.join("data");
    let tmp = root.join("tmp");
    let bin = root.join("bin");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(&tmp).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let canonical_root = fs::canonicalize(&root).unwrap();
    let command = if platform == "launchd" {
        "launchctl"
    } else {
        "systemctl"
    };
    fs::write(bin.join(command), "#!/bin/sh\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(bin.join(command), fs::Permissions::from_mode(0o700)).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"))
        .args(["mcp", "--stdio"])
        .current_dir(&root)
        .env_clear()
        .env("PATH", &bin)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("TMPDIR", &tmp)
        .env("SYMERASEME_DATA_DIR", &data)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{}\n", case["request"].as_str().unwrap()).as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let actual = String::from_utf8(output.stdout)
        .unwrap()
        .replace(canonical_root.to_string_lossy().as_ref(), "<SCHEDULE_ROOT>")
        .replace(root.to_string_lossy().as_ref(), "<SCHEDULE_ROOT>");
    assert_eq!(
        actual,
        case["response"].as_str().unwrap(),
        "{}",
        case["name"]
    );

    let native_units = match platform {
        "launchd" => home.join("Library/LaunchAgents"),
        "systemd" => home.join("config/systemd/user"),
        _ => unreachable!(),
    };
    assert!(native_units.is_dir(), "{}", case["name"]);
    assert!(
        fs::read_dir(native_units).unwrap().next().is_some(),
        "{}",
        case["name"]
    );
    fs::remove_dir_all(root).unwrap();
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
