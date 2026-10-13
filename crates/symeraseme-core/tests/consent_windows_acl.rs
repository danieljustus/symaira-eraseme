//! Native Windows files and DACLs compared with authenticated retained Go observations.
#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

#[path = "support/frozen_consent_acl.rs"]
mod frozen_consent_acl;

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use symeraseme_core::identity::{ConsentError, ConsentStore};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn capture(mut command: Command, root: &Path, label: &str, budget: Duration) -> Output {
    let stdout = root.join(format!("{label}.stdout"));
    let stderr = root.join(format!("{label}.stderr"));
    let mut child = command
        .stdin(Stdio::null())
        .stdout(fs::File::create(&stdout).unwrap())
        .stderr(fs::File::create(&stderr).unwrap())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + budget;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            child.wait().unwrap();
            panic!("native consent helper exceeded its deadline; private diagnostics retained");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    child.wait().unwrap();
    for path in [&stdout, &stderr] {
        assert!(fs::metadata(path).unwrap().len() <= 64 * 1024);
    }
    Output {
        status,
        stdout: fs::read(stdout).unwrap(),
        stderr: fs::read(stderr).unwrap(),
    }
}

fn private(command: &mut Command, root: &Path) {
    let home = root.join("home");
    let temp = root.join("tmp");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&temp).unwrap();
    command
        .env_clear()
        .current_dir(root)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("TEMP", &temp)
        .env("TMP", &temp);
    for key in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

fn acl(path: &Path, mode: &str, logs: &Path, label: &str) -> Value {
    let executable = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let mut command = Command::new(executable);
    private(&mut command, logs);
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(repo().join("crates/symeraseme-core/tests/support/consent_acl.ps1"))
        .arg("-Path")
        .arg(path)
        .arg("-Mode")
        .arg(mode);
    let result = capture(command, logs, label, Duration::from_secs(30));
    assert!(result.status.success(), "native ACL observation failed");
    assert!(
        result.stderr
            == format!(
                "consent ACL script started: {mode}\r\nconsent ACL script completed: {mode}\r\n"
            )
            .as_bytes()
    );
    serde_json::from_slice(&result.stdout).unwrap()
}

// Return only fixed labels, never the private PowerShell diagnostic or its values.
fn identity_failure_label(stderr: &[u8]) -> &'static str {
    let text = String::from_utf8_lossy(stderr);
    [
        "source_revision_matches_head",
        "source_tree_clean",
        "native_architecture",
        "user_matches_launch_process_token",
        "user_sid_observed",
        "primary_group_sid_observed",
        "rid500_local_account_unique",
        "rid500_matches_sddl_la",
        "baseline_schema",
        "baseline_revision_matches",
        "baseline_script_matches",
        "baseline_native_host_matches",
        "user_sid_matches_baseline",
        "primary_group_sid_matches_baseline",
        "rid500_sid_matches_baseline",
        "sddl_la_matches_baseline",
        "baseline artifact missing",
        "baseline artifact too large",
        "Get-CimInstance",
        "Get-FileHash",
        "Add-Type",
        "CommandNotFoundException",
        "ParameterBindingValidationException",
        "PSSecurityException",
        "running scripts is disabled",
        "is not digitally signed",
        "UnauthorizedAccess",
        "not recognized as the name of a script file",
        "Failed to create CoreCLR",
    ]
    .into_iter()
    .find(|label| text.contains(label))
    .unwrap_or("unclassified PowerShell failure")
}

// Read-only engine probe, without paths, baseline contents or a policy override.
const IDENTITY_ENGINE_PROBE: &str = r#"
$ErrorActionPreference = 'Stop'
Write-Output 'engine-started'
Write-Output ('policy:' + [string](Get-ExecutionPolicy))
Write-Output ('cim:' + [bool](Get-Command Get-CimInstance -ErrorAction SilentlyContinue))
Write-Output ('git:' + [bool](Get-Command git -CommandType Application -ErrorAction SilentlyContinue))
"#;

// Optional diagnostics must not replace the verifier's original fatal result.
fn optional_identity_probe(
    probe: impl FnOnce() -> Output + std::panic::UnwindSafe,
) -> Option<Output> {
    std::panic::catch_unwind(probe).ok()
}

#[test]
fn identity_probe_failure_returns_to_original_verifier() {
    assert!(optional_identity_probe(|| panic!("synthetic capture failure")).is_none());
}

fn identity_probe_markers(stdout: &[u8]) -> Vec<&'static str> {
    let text = String::from_utf8_lossy(stdout);
    [
        "engine-started",
        "policy:Restricted",
        "policy:AllSigned",
        "policy:RemoteSigned",
        "policy:Unrestricted",
        "policy:Bypass",
        "policy:Undefined",
        "cim:True",
        "cim:False",
        "git:True",
        "git:False",
    ]
    .into_iter()
    .filter(|marker| text.lines().any(|line| line == *marker))
    .collect()
}

#[test]
fn identity_failure_diagnostics_are_fixed_labels_only() {
    assert_eq!(
        identity_failure_label(b"private-prefix check failed: source_tree_clean private-suffix"),
        "source_tree_clean"
    );
    assert_eq!(
        identity_failure_label(b"Get-CimInstance: private details"),
        "Get-CimInstance"
    );
    for input in [b"private-value".as_slice(), b"", b"\xff\xfe private-value"] {
        assert_eq!(
            identity_failure_label(input),
            "unclassified PowerShell failure"
        );
        assert!(identity_probe_markers(input).is_empty());
    }
    assert_eq!(
        identity_failure_label(b"private-path: running scripts is disabled on this system"),
        "running scripts is disabled"
    );
    assert_eq!(
        identity_probe_markers(
            b"engine-started\r\npolicy:Restricted\r\ncim:False\r\ngit:True\r\nprivate-value\n"
        ),
        [
            "engine-started",
            "policy:Restricted",
            "cim:False",
            "git:True"
        ]
    );
    assert!(
        identity_probe_markers(b"policy:Restricted private-value\ncim:True private-value")
            .is_empty()
    );
}

#[cfg(windows)]
fn verified_primary_group(logs: &Path, arch: &str) -> String {
    let baseline_path = PathBuf::from(
        std::env::var_os("SYMERASEME_WINDOWS_IDENTITY_BASELINE")
            .expect("fresh Windows identity baseline is required"),
    );
    assert!(baseline_path.is_absolute() && fs::metadata(&baseline_path).unwrap().len() <= 16384);
    let before = fs::read(&baseline_path).unwrap();
    assert!(before.len() <= 16384);
    let baseline: Value = serde_json::from_slice(&before).unwrap();
    let programs = PathBuf::from(std::env::var_os("ProgramFiles").expect("native ProgramFiles"));
    let powershell = programs.join("PowerShell/7/pwsh.exe");
    let git = programs.join("Git/cmd/git.exe");
    assert!(
        powershell.is_file() && git.is_file(),
        "trusted native executables unavailable"
    );
    let mut revision = Command::new(&git);
    private(&mut revision, logs);
    revision.arg("-C").arg(repo()).args(["rev-parse", "HEAD"]);
    let output = capture(revision, logs, "identity-source", Duration::from_secs(30));
    assert!(output.status.success());
    let revision = std::str::from_utf8(&output.stdout).unwrap().trim();
    let script = repo().join("scripts/capture-windows-identity.ps1");
    let script_bytes = fs::read(&script).unwrap();
    let identity_command = || {
        let mut command = Command::new(&powershell);
        private(&mut command, logs);
        // Both commands use exactly the same isolated environment: PATH for Git,
        // PSModulePath for CIM, ProgramFiles for the shared verifier's Git Bash.
        for key in ["PATH", "PSModulePath", "ProgramFiles"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
    };
    let mut verify = identity_command();
    verify
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
        .arg(&script)
        .args([
            "-Mode",
            "Verify",
            "-ExpectedArch",
            arch,
            "-ExpectedRevision",
            revision,
            "-ArtifactPath",
        ])
        .arg(&baseline_path);
    let result = capture(verify, logs, "identity-verify", Duration::from_secs(60));
    if !result.status.success() {
        eprintln!(
            "identity verifier diagnostic: exit {:?}, stderr bytes {}, stderr contains NUL {}, stdout class {}, stderr class {}",
            result.status.code(),
            result.stderr.len(),
            result.stderr.contains(&0),
            identity_failure_label(&result.stdout),
            identity_failure_label(&result.stderr)
        );
        let probe = optional_identity_probe(|| {
            let mut probe = identity_command();
            probe.args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                IDENTITY_ENGINE_PROBE,
            ]);
            capture(probe, logs, "identity-engine", Duration::from_secs(30))
        });
        if let Some(probe) = probe {
            eprintln!(
                "identity engine diagnostic: exit {:?}, markers {:?}",
                probe.status.code(),
                identity_probe_markers(&probe.stdout)
            );
        } else {
            eprintln!("identity engine diagnostic: probe unavailable; original failure retained");
        }
    }
    assert!(
        result.status.success(),
        "independent native identity verification failed (exit {:?}, class: {})",
        result.status.code(),
        identity_failure_label(&result.stderr)
    );
    assert!(
        fs::read(&baseline_path).unwrap() == before,
        "identity baseline changed during verification"
    );
    assert!(
        fs::read(&script).unwrap() == script_bytes,
        "identity verifier changed"
    );
    frozen_consent_acl::verified_group(
        &baseline,
        revision,
        &hex::encode(Sha256::digest(script_bytes)),
        arch,
    )
}

#[cfg(not(windows))]
#[test]
fn consent_acl_capture_fails_off_windows_when_requested() {
    frozen_consent_acl::reject_retired_modes();
}

#[cfg(windows)]
#[test]
fn native_consent_files_and_dacls_match_recorded_go() {
    use frozen_consent_acl::{assert_acl, assert_files};
    use std::collections::BTreeMap;
    frozen_consent_acl::reject_retired_modes();
    let root = tempfile::tempdir().unwrap();
    let (target, arch) = match std::env::consts::ARCH {
        "x86_64" => ("windows/amd64", "X64"),
        "aarch64" => ("windows/arm64", "ARM64"),
        _ => panic!("unsupported native Windows architecture"),
    };
    let record = frozen_consent_acl::load(target);
    let group = verified_primary_group(root.path(), arch);
    let mut completed = 0;

    for (case, protect, readonly_dir, readonly_token) in [
        ("inherited", false, false, false),
        ("owner-only-parent", true, false, false),
        ("readonly-directory", true, true, false),
        ("readonly-token", true, false, true),
    ] {
        let parent = root.path().join(case);
        fs::create_dir(&parent).unwrap();
        if protect {
            let observed = acl(&parent, "protect", root.path(), case);
            assert!(observed["protected"] == true);
            assert_acl(&record.acl(case, &group), &observed);
        }
        let parent_label = format!("parent-{case}");
        assert_acl(
            &record.acl(&parent_label, &group),
            &acl(&parent, "read", root.path(), &parent_label),
        );
        let rust_dir = parent.join("rust");
        fs::create_dir(&rust_dir).unwrap();
        fs::write(rust_dir.join("sentinel"), "unrelated sentinel").unwrap();
        let directory_permissions = fs::metadata(&rust_dir).unwrap().permissions();
        if readonly_dir {
            let mut p = directory_permissions.clone();
            p.set_readonly(true);
            fs::set_permissions(&rust_dir, p).unwrap();
        }
        let store = ConsentStore::new(&rust_dir)
            .with_clock(|| 1000)
            .with_random_source(|length| Ok(vec![7; length]));
        store.issue_token("before", 60).unwrap();
        let mut failed = false;
        if readonly_token {
            for entry in fs::read_dir(&rust_dir).unwrap() {
                let path = entry.unwrap().path();
                if path.file_name().unwrap() == "sentinel" {
                    continue;
                }
                let mut p = fs::metadata(&path).unwrap().permissions();
                p.set_readonly(true);
                fs::set_permissions(path, p).unwrap();
            }
            let error = store.issue_token("after", 60).unwrap_err();
            assert!(
                matches!(error, ConsentError::Io(error) if error.kind() == std::io::ErrorKind::PermissionDenied)
            );
            failed = true;
        }
        record.assert_outcome(case, failed);
        let observed_dir = acl(&rust_dir, "read", root.path(), &format!("rust-dir-{case}"));
        for implementation in ["go", "rust"] {
            assert_acl(
                &record.acl(&format!("{implementation}-dir-{case}"), &group),
                &observed_dir,
            );
        }
        let mut names = fs::read_dir(&rust_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        names.sort();
        let expected = record.files(case);
        let actual: BTreeMap<_, _> = names
            .iter()
            .map(|name| (name.clone(), fs::read(rust_dir.join(name)).unwrap()))
            .collect();
        assert_files(&expected, &actual);
        for (index, name) in names.into_iter().enumerate() {
            let rust = rust_dir.join(&name);
            let observed = acl(&rust, "read", root.path(), &format!("file-{case}-{index}"));
            let role = if name == "sentinel" {
                "sentinel"
            } else {
                frozen_consent_acl::NAME_ROLE
            };
            for implementation in ["go", "rust"] {
                assert_acl(
                    &record.acl(&format!("{implementation}-file-{case}-{role}"), &group),
                    &observed,
                );
            }
            // Undo only our own read-only control before checked cleanup.
            #[allow(clippy::permissions_set_readonly_false)]
            {
                let mut p = fs::metadata(&rust).unwrap().permissions();
                p.set_readonly(false);
                fs::set_permissions(rust, p).unwrap();
            }
        }
        fs::set_permissions(&rust_dir, directory_permissions).unwrap();
        completed += 1;
        eprintln!(
            "native consent {case}: retained Go bytes, full security descriptor and sentinel matched by Rust"
        );
    }
    assert_eq!(completed, 4, "all native consent cases executed");
    root.close().unwrap();
}
