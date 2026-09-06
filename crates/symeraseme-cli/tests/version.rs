use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    run_with_version_env(args, None)
}

fn run_with_version_env(args: &[&str], version: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"));
    if let Some(version) = version {
        command.env("SYMERASEME_VERSION", version);
    }
    command
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
fn root_version_short_flag_matches_go_bytes() {
    assert_success(&["-v"], b"symeraseme version 0.13.0\n");
}

#[test]
fn unsupported_root_version_short_flag_matches_go_bytes() {
    let output = run(&["-V"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"unknown shorthand flag: 'V' in -V\n");
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
fn version_rejects_extra_positional_arguments_with_go_error() {
    let expected = b"unknown command \"extra\" for \"symeraseme version\"\n";
    for args in [
        &["version", "extra"][..],
        &["version", "--json", "extra"][..],
    ] {
        let output = run(args);
        assert_eq!(output.status.code(), Some(1), "args: {args:?}");
        assert!(output.stdout.is_empty(), "args: {args:?}");
        assert_eq!(output.stderr, expected, "args: {args:?}");
    }
}

#[test]
fn root_version_ignores_trailing_argument_like_go() {
    assert_success(&["--version", "extra"], b"symeraseme version 0.13.0\n");
}

#[test]
fn poisoned_environment_cannot_override_any_version_surface() {
    let cases = [
        (
            &["--version"][..],
            0,
            b"symeraseme version 0.13.0\n".as_slice(),
            b"".as_slice(),
        ),
        (
            &["-v"][..],
            0,
            b"symeraseme version 0.13.0\n".as_slice(),
            b"".as_slice(),
        ),
        (
            &["version"][..],
            0,
            b"symeraseme 0.13.0\n".as_slice(),
            b"".as_slice(),
        ),
        (
            &["version", "--json"][..],
            0,
            b"{\"tool\":\"symeraseme\",\"version\":\"0.13.0\",\"schema_version\":1}\n".as_slice(),
            b"".as_slice(),
        ),
    ];

    for (args, expected_code, expected_stdout, expected_stderr) in cases {
        let output = run_with_version_env(args, Some("9.9.9"));
        assert_eq!(output.status.code(), Some(expected_code), "args: {args:?}");
        assert_eq!(output.stdout, expected_stdout, "args: {args:?}");
        assert_eq!(output.stderr, expected_stderr, "args: {args:?}");
    }
}
