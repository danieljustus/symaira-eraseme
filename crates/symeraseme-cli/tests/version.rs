use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"))
        .args(args)
        .output()
        .expect("symeraseme-rust should launch")
}

fn assert_success(args: &[&str], expected_stdout: &[u8]) {
    let output = run(args);
    assert_eq!(output.status.code(), Some(0), "stderr: {:?}", output.stderr);
    assert_eq!(output.stdout, expected_stdout);
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {:?}",
        output.stderr
    );
}

#[test]
fn root_version_matches_go_bytes() {
    assert_success(&["--version"], b"symeraseme version 0.13.0\n");
}

#[test]
fn version_text_matches_go_bytes() {
    assert_success(&["version"], b"symeraseme 0.13.0\n");
}

#[test]
fn version_json_matches_handshake_bytes() {
    assert_success(
        &["version", "--json"],
        b"{\"tool\":\"symeraseme\",\"version\":\"0.13.0\",\"schema_version\":1}\n",
    );
}

#[test]
fn version_rejects_extra_positional_arguments() {
    let output = run(&["version", "extra"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("symeraseme-rust"),
        "internal binary name leaked: {:?}",
        output.stderr
    );
}

#[test]
fn root_version_rejects_extra_positional_arguments() {
    let output = run(&["--version", "extra"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}
