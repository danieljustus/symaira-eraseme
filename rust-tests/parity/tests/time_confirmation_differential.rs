use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use symeraseme_core::confirmation::extract_confirmation_links;
use symeraseme_core::timeutil::{TimestampError, format_iso, format_sql, format_sql_bytes, parse};

const ORACLE_TIMEOUT: Duration = Duration::from_secs(30);

struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn capture(mut command: Command, directory: &Path, label: &str, deadline: Instant) -> Vec<u8> {
    let stdout = directory.join(format!("{label}.stdout"));
    let stderr = directory.join(format!("{label}.stderr"));
    let mut child = ChildGuard(
        command
            .stdin(Stdio::null())
            .stdout(std::fs::File::create(&stdout).unwrap())
            .stderr(std::fs::File::create(&stderr).unwrap())
            .spawn()
            .expect("start live Go oracle"),
    );
    let status = loop {
        if let Some(status) = child.0.try_wait().expect("read oracle status") {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "live Go oracle exceeded unchanged total 30-second budget"
        );
        thread::sleep(Duration::from_millis(10));
    };
    child.0.wait().unwrap();
    assert!(std::fs::metadata(&stdout).unwrap().len() <= 1024 * 1024);
    assert!(std::fs::metadata(&stderr).unwrap().len() <= 64 * 1024);
    assert!(
        status.success(),
        "live Go oracle failed; private bounded diagnostics retained until cleanup"
    );
    std::fs::read(stdout).unwrap()
}

fn run_go_oracle() -> Value {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let private = tempfile::tempdir().unwrap();
    let deadline = Instant::now() + ORACLE_TIMEOUT;
    let mut query = Command::new("go");
    query
        .args(["env", "GOVERSION", "GOMODCACHE", "GOCACHE"])
        .env("GOENV", "off")
        .env("GOTOOLCHAIN", "local")
        .env("GOWORK", "off");
    let query_bytes = capture(query, private.path(), "cache-query", deadline);
    let query_text = std::str::from_utf8(&query_bytes).unwrap();
    let fields: Vec<_> = query_text.lines().collect();
    assert_eq!(fields.len(), 3);
    assert_eq!(
        fields[0], "go1.26.6",
        "live capture requires the pinned Go toolchain"
    );
    let mut build = Command::new("go");
    build
        .args(["build", "-mod=readonly", "-buildvcs=true", "-o"])
        .arg(
            private
                .path()
                .join(format!("oracle{}", std::env::consts::EXE_SUFFIX)),
        )
        .arg("./rust-tests/parity/oracle")
        .current_dir(&repository);
    let configure = |command: &mut Command| {
        let path = std::env::var_os("PATH").expect("live Go requires PATH");
        let system_root = std::env::var_os("SystemRoot");
        command.env_clear().env("PATH", path);
        if let Some(system_root) = system_root {
            command.env("SystemRoot", system_root);
        }
        for key in [
            "HOME",
            "USERPROFILE",
            "XDG_CONFIG_HOME",
            "XDG_CACHE_HOME",
            "XDG_DATA_HOME",
            "TMPDIR",
            "TMP",
            "TEMP",
            "SYMERASEME_DATA_DIR",
        ] {
            command.env(key, private.path());
        }
        command
            .env("GOENV", "off")
            .env("GOWORK", "off")
            .env("GOTOOLCHAIN", "local")
            .env("GOPROXY", "off")
            .env("GOSUMDB", "off")
            .env("CGO_ENABLED", "0")
            .env("GOMODCACHE", fields[1])
            .env("GOCACHE", fields[2])
            .env("TZ", "UTC");
    };
    configure(&mut build);
    capture(build, private.path(), "build", deadline);
    let mut run = Command::new(
        private
            .path()
            .join(format!("oracle{}", std::env::consts::EXE_SUFFIX)),
    );
    run.current_dir(private.path());
    configure(&mut run);
    serde_json::from_slice(&capture(run, private.path(), "run", deadline)).unwrap()
}

fn frozen_go_oracle() -> Value {
    use sha2::{Digest, Sha256};
    let raw = include_bytes!("../fixtures/frozen/go1.26.6/time-confirmation/observations.json");
    let provenance: Value = serde_json::from_slice(include_bytes!(
        "../fixtures/frozen/go1.26.6/time-confirmation/provenance.json"
    ))
    .unwrap();
    assert_eq!(provenance["exit_status"], 0);
    assert_eq!(provenance["go_version"], "go version go1.26.6 linux/amd64");
    assert_eq!(provenance["counts"]["timestamps"], 32);
    assert_eq!(provenance["counts"]["urls"], 8);
    assert_eq!(provenance["stdout"]["bytes"], raw.len());
    assert_eq!(provenance["stdout"]["sha256"], hex(&Sha256::digest(raw)));
    let input = include_bytes!("../oracle/cases.json");
    assert_eq!(
        provenance["sources"]["rust-tests/parity/oracle/cases.json"]["sha256"],
        hex(&Sha256::digest(input))
    );
    serde_json::from_slice(raw).unwrap()
}

fn string_field<'a>(row: &'a Value, name: &str) -> &'a str {
    row.get(name).and_then(Value::as_str).unwrap_or("")
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

#[test]
fn go_oracle_matches_rust_time_and_confirmation_contract() {
    let fixture: Value = serde_json::from_slice(include_bytes!("../oracle/cases.json"))
        .expect("committed parity fixture must be valid JSON");
    let oracle = match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
        Ok("1") => run_go_oracle(),
        Ok("0") | Err(std::env::VarError::NotPresent) => frozen_go_oracle(),
        _ => panic!("SYMERASEME_PARITY_LIVE_GO must be 0 or 1"),
    };
    assert_observations_match(&fixture, &oracle);
}

fn assert_observations_match(fixture: &Value, oracle: &Value) {
    let fixture_timestamps = fixture["timestamps"]
        .as_array()
        .expect("timestamp fixture rows");
    let oracle_timestamps = oracle["timestamps"]
        .as_array()
        .expect("timestamp oracle rows");
    assert!(
        fixture_timestamps.len() == oracle_timestamps.len(),
        "timestamp fixture and oracle row counts differ"
    );

    for (index, (fixture_row, oracle_row)) in
        fixture_timestamps.iter().zip(oracle_timestamps).enumerate()
    {
        let input = fixture_row["input"]
            .as_str()
            .expect("timestamp fixture input");
        assert!(
            string_field(oracle_row, "input") == input,
            "timestamp oracle input changed at row {index}"
        );
        let accepted = oracle_row["accepted"]
            .as_bool()
            .expect("timestamp oracle acceptance");
        match parse(input) {
            Ok(value) => {
                assert!(
                    accepted,
                    "Rust accepted timestamp rejected by Go at row {index}"
                );
                assert!(
                    string_field(oracle_row, "iso") == format_iso(value),
                    "canonical ISO differs at timestamp row {index}"
                );
                assert!(
                    string_field(oracle_row, "sql") == format_sql(value),
                    "canonical SQL differs at timestamp row {index}"
                );
                assert!(
                    string_field(oracle_row, "sql_bytes_hex") == hex(&format_sql_bytes(value)),
                    "canonical SQL bytes differ at timestamp row {index}"
                );
            }
            Err(error) => {
                assert!(
                    !accepted,
                    "Rust rejected timestamp accepted by Go at row {index}"
                );
                let class = match error {
                    TimestampError::Empty => "empty",
                    TimestampError::Malformed => "malformed",
                };
                assert!(
                    string_field(oracle_row, "error_class") == class,
                    "timestamp error class differs at row {index}"
                );
            }
        }
    }

    let fixture_urls = fixture["urls"].as_array().expect("URL fixture rows");
    let oracle_urls = oracle["urls"].as_array().expect("URL oracle rows");
    assert!(
        fixture_urls.len() == oracle_urls.len(),
        "URL fixture and oracle row counts differ"
    );
    for (index, (fixture_row, oracle_row)) in fixture_urls.iter().zip(oracle_urls).enumerate() {
        let input = fixture_row["input"].as_str().expect("URL fixture input");
        assert!(
            string_field(oracle_row, "input") == input,
            "URL oracle input changed at row {index}"
        );
        let expected = oracle_row["links"].as_array().expect("URL oracle links");
        let actual = extract_confirmation_links(input);
        assert!(
            expected.len() == actual.len(),
            "URL result count differs at row {index}"
        );
        for (link_index, (expected_link, actual_link)) in expected.iter().zip(actual).enumerate() {
            assert!(
                expected_link.as_str() == Some(actual_link.as_str()),
                "URL result differs at row {index}, link {link_index}"
            );
        }
    }
}

#[test]
fn frozen_oracle_mutations_are_rejected_without_reducing_case_counts() {
    let fixture: Value = serde_json::from_slice(include_bytes!("../oracle/cases.json")).unwrap();
    let oracle = frozen_go_oracle();
    let mut changed = oracle.clone();
    changed["timestamps"][0]["iso"] = Value::String("corrupted-observation".into());
    assert!(std::panic::catch_unwind(|| assert_observations_match(&fixture, &changed)).is_err());
    let mut changed = oracle.clone();
    changed["timestamps"].as_array_mut().unwrap().pop();
    assert!(std::panic::catch_unwind(|| assert_observations_match(&fixture, &changed)).is_err());
    let mut changed = oracle;
    changed["urls"][0]["links"]
        .as_array_mut()
        .unwrap()
        .push(Value::String(
            "https://mutation.example.invalid/confirm".into(),
        ));
    assert!(std::panic::catch_unwind(|| assert_observations_match(&fixture, &changed)).is_err());
}
