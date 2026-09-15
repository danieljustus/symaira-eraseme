use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ISOLATION: AtomicU64 = AtomicU64::new(0);

const KEY: [u8; 32] = [0x42; 32];
const WRONG_KEY: [u8; 32] = [0x43; 32];
const HEADER: &[u8] =
    br#"{"version":2,"nonce":"000000000000000000000000","algorithm":"AES-256-GCM"}"#;
const PROFILE_JSON: &[u8] = br#"{
  "full_name": "Test Person",
  "name_variants": ["Alias"],
  "date_of_birth": "1980-01-02",
  "addresses": [{"street":"Main Street","city":"Berlin","postal_code":"10115","country":"DE","state":"BE","valid_from":null,"valid_to":null}],
  "email_addresses": ["test@example.test", "second@example.test"],
  "phone_numbers": ["+1-555-0100"],
  "jurisdictions": ["DE", "EU"],
  "secret_marker": "PRIVATE_PROFILE_FIELD"
}"#;
const EXPECTED_JSON: &[u8] = br#"{"full_name":"Test Person","name_variants":["Alias"],"date_of_birth":"1980-01-02","addresses":[{"street":"Main Street","city":"Berlin","postal_code":"10115","country":"DE","state":"BE","valid_from":null,"valid_to":null}],"email_addresses":["test@example.test","second@example.test"],"phone_numbers":["+1-555-0100"],"jurisdictions":["DE","EU"]}
"#;
const EXPECTED_TEXT: &[u8] = b"Name: Test Person\nEmail: test@example.test\nEmail: second@example.test\nJurisdiction: DE\nJurisdiction: EU\n";

struct Isolation {
    root: PathBuf,
    profile: PathBuf,
}

impl Isolation {
    fn new() -> Self {
        let id = NEXT_ISOLATION.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "symeraseme-show-profile-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("home")).unwrap();
        fs::create_dir_all(root.join("config")).unwrap();
        fs::create_dir_all(root.join("data")).unwrap();
        fs::create_dir_all(root.join("tmp")).unwrap();
        Self {
            profile: root.join("data/identity.encrypted"),
            root,
        }
    }

    fn write_encrypted_profile(&self, key: &[u8; 32]) {
        let cipher = Aes256Gcm::new_from_slice(key).unwrap();
        let nonce = [0_u8; 12];
        let ciphertext = cipher
            .encrypt(
                &Nonce::try_from(nonce.as_slice()).unwrap(),
                Payload {
                    msg: PROFILE_JSON,
                    aad: HEADER,
                },
            )
            .unwrap();
        let mut raw = HEADER.to_vec();
        raw.push(b'\n');
        raw.extend_from_slice(&ciphertext);
        fs::write(&self.profile, raw).unwrap();
    }

    fn write_raw_profile(&self, raw: &[u8]) {
        fs::write(&self.profile, raw).unwrap();
    }

    fn run(&self, args: &[&str], key: Option<&[u8; 32]>, disable_keyring: bool) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"));
        command
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("USERPROFILE", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("TMPDIR", self.root.join("tmp"))
            .env("SYMERASEME_DATA_DIR", self.root.join("data"))
            .env("SYMERASEME_IDENTITY_PATH", &self.profile)
            .env("RUST_BACKTRACE", "0")
            .args(args);
        if let Some(key) = key {
            command.env("SYMERASEME_IDENTITY_MASTER_KEY", hex::encode(key));
        } else {
            command.env_remove("SYMERASEME_IDENTITY_MASTER_KEY");
        }
        if disable_keyring {
            command.env("SYMERASEME_DISABLE_KEYRING", "1");
        } else {
            command.env_remove("SYMERASEME_DISABLE_KEYRING");
        }
        command.output().unwrap()
    }
}

impl Drop for Isolation {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_success(output: &Output, stdout: &[u8]) {
    assert_eq!(output.status.code(), Some(0), "stderr: {:?}", output.stderr);
    assert_eq!(output.stdout, stdout);
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {:?}",
        output.stderr
    );
}

fn assert_failure(output: &Output, stderr: &[u8]) {
    assert_eq!(output.status.code(), Some(1), "stdout: {:?}", output.stdout);
    assert!(
        output.stdout.is_empty(),
        "unexpected stdout: {:?}",
        output.stdout
    );
    assert_eq!(output.stderr, stderr);
}

#[test]
fn show_profile_text_matches_go() {
    let isolation = Isolation::new();
    isolation.write_encrypted_profile(&KEY);
    let output = isolation.run(&["show-profile"], Some(&KEY), false);
    assert_success(&output, EXPECTED_TEXT);
    assert!(
        !output
            .stdout
            .windows(b"PRIVATE_PROFILE_FIELD".len())
            .any(|window| { window == b"PRIVATE_PROFILE_FIELD" })
    );
    assert!(
        !output
            .stdout
            .windows(hex::encode(KEY).len())
            .any(|window| { window == hex::encode(KEY).as_bytes() })
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn show_profile_json_matches_go() {
    let isolation = Isolation::new();
    isolation.write_encrypted_profile(&KEY);
    let output = isolation.run(&["show-profile", "--output", "json"], Some(&KEY), false);
    assert_success(&output, EXPECTED_JSON);
    assert!(
        !output
            .stdout
            .windows(b"PRIVATE_PROFILE_FIELD".len())
            .any(|window| { window == b"PRIVATE_PROFILE_FIELD" })
    );
    assert!(
        !output
            .stdout
            .windows(hex::encode(KEY).len())
            .any(|window| { window == hex::encode(KEY).as_bytes() })
    );
}

#[test]
fn show_profile_missing_profile_matches_go() {
    let isolation = Isolation::new();
    let output = isolation.run(&["show-profile", "--output", "json"], Some(&KEY), false);
    assert_failure(&output, b"identity: profile not found\n");
}

#[test]
fn show_profile_corrupt_profile_matches_go() {
    let isolation = Isolation::new();
    isolation.write_raw_profile(b"not-an-envelope");
    let output = isolation.run(&["show-profile"], Some(&KEY), true);
    assert_failure(&output, b"identity: profile corrupt: no header separator\n");
}

#[test]
fn show_profile_wrong_key_matches_go() {
    let isolation = Isolation::new();
    isolation.write_encrypted_profile(&KEY);
    let output = isolation.run(&["show-profile"], Some(&WRONG_KEY), true);
    assert_failure(
        &output,
        b"identity: profile corrupt: cipher: message authentication failed\n",
    );
}

#[test]
fn show_profile_missing_key_does_not_touch_keyring() {
    let isolation = Isolation::new();
    isolation.write_encrypted_profile(&KEY);
    let output = isolation.run(&["show-profile", "--output", "json"], None, true);
    assert_failure(&output, b"identity: master key missing\n");
}
