use serde_json::Value;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use symeraseme_core::confirmation::extract_confirmation_links;
use symeraseme_core::timeutil::{TimestampError, format_iso, format_sql, format_sql_bytes, parse};

const ORACLE_TIMEOUT: Duration = Duration::from_secs(30);

fn run_go_oracle() -> Value {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut child = Command::new("go")
        .args(["run", "./rust-tests/parity/oracle"])
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Go must be available for the committed parity oracle");
    let deadline = Instant::now() + ORACLE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                assert!(status.success(), "Go parity oracle exited unsuccessfully");
                let output = child
                    .wait_with_output()
                    .expect("Go parity oracle output must be readable");
                return serde_json::from_slice(&output.stdout)
                    .expect("Go parity oracle must emit valid JSON");
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("Go parity oracle exceeded its bounded timeout");
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("Go parity oracle status could not be read");
            }
        }
    }
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
    let oracle = run_go_oracle();
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
