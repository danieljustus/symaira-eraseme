//! Shared bounded runner for the committed Go oracles.
//!
//! `go run .` recompiles the oracle on every invocation. The native parity job
//! ran several of those inside a 30-second execution deadline and timed out
//! (#912). This builds the oracle once per test process into a private
//! temporary directory and executes the compiled binary, so the compile cost
//! leaves the runtime budget instead of eating it.
//!
//! Output is redirected to files rather than pipes: the caller keeps ownership
//! of the child, so a timeout can actually kill it, and a large oracle payload
//! cannot deadlock against a full pipe buffer.

// The module is included per test file via `#[path]`, so each file sees every
// helper while using only some of them.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Runtime budget for one oracle invocation.
pub const ORACLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Separate, larger budget for the one-off build: compiling costs more than
/// running, and the cold-cache case must not be mistaken for a hang.
pub const ORACLE_BUILD_TIMEOUT: Duration = Duration::from_secs(120);

/// The Go toolchain the committed oracles are pinned to. CI pins the same
/// version in `rust-ci.yml`.
pub const GO_TOOLCHAIN: &str = "go1.26.6";

/// What an oracle invocation produced.
pub struct OracleRun {
    pub status: std::process::ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

fn registry() -> &'static Mutex<HashMap<&'static str, PathBuf>> {
    static REGISTRY: OnceLock<Mutex<HashMap<&'static str, PathBuf>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scratch_root(package: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "symeraseme-oracle-{}-{}",
        std::process::id(),
        package.replace('/', "-")
    ));
    std::fs::create_dir_all(&root).expect("create oracle scratch directory");
    root
}

/// Builds `rust-tests/parity/oracle/<package>` once per test process and
/// returns the compiled executable.
fn oracle_executable(package: &'static str) -> PathBuf {
    let mut registry = registry().lock().expect("oracle registry");
    if let Some(executable) = registry.get(package) {
        return executable.clone();
    }
    let root = repository_root();
    let scratch = scratch_root(package);
    let name = package.rsplit('/').next().expect("oracle package name");
    let executable = scratch.join(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    });

    let mut build = Command::new("go");
    build
        .current_dir(&root)
        .args(["build", "-o"])
        .arg(&executable)
        .arg(format!("./rust-tests/parity/oracle/{package}"))
        // Go's out-of-range float-to-int conversion is target-specific.
        .env("GOARCH", go_arch())
        .env("GOWORK", "off");
    let run = run_bounded(
        build,
        None,
        &scratch.join("build.stdout"),
        &scratch.join("build.stderr"),
        ORACLE_BUILD_TIMEOUT,
    )
    .expect("Go must be available for the committed oracle");
    assert!(
        run.status.success(),
        "Go oracle build failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    registry.insert(package, executable.clone());
    executable
}

fn go_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        arch => panic!("Go parity oracle has no target mapping for Rust architecture {arch}"),
    }
}

/// Runs the named oracle with an optional stdin payload, building it first if
/// this process has not done so yet. Both phases are bounded.
///
/// Tests in a file run concurrently, so each invocation writes to its own
/// output files. Sharing them let two concurrent oracles overwrite each other's
/// payload mid-read, which surfaced as a corrupt-header failure rather than as
/// anything resembling a race.
pub fn run_oracle(package: &'static str, stdin: Option<&[u8]>) -> OracleRun {
    let executable = oracle_executable(package);
    let scratch = scratch_root(package);
    let invocation = INVOCATION.fetch_add(1, Ordering::Relaxed);
    let mut command = Command::new(executable);
    command.current_dir(repository_root());
    run_bounded(
        command,
        stdin,
        &scratch.join(format!("run-{invocation}.stdout")),
        &scratch.join(format!("run-{invocation}.stderr")),
        ORACLE_TIMEOUT,
    )
    .expect("Go oracle execution must complete within its bounded timeout")
}

/// Distinguishes concurrent invocations of the same oracle within one process.
static INVOCATION: AtomicUsize = AtomicUsize::new(0);

/// A `go` invocation with the repository as its working directory and the
/// isolated build cache the storage oracle needs. Exposed so callers that run
/// `go test` rather than a built binary share the same bounded execution.
pub fn go_command(repository: &Path, build_cache: &Path) -> Command {
    let mut command = Command::new("go");
    command
        .current_dir(repository)
        .env("GOCACHE", build_cache)
        .env("GOTOOLCHAIN", GO_TOOLCHAIN)
        .env("GOWORK", "off");
    command
}

/// Builds the named oracle with extra build tags once per test process and runs
/// it. Needed by the storage oracle, which is guarded by `storage_oracle`.
pub fn run_oracle_with_tags(
    package: &'static str,
    tags: &[&str],
    build_cache: &Path,
    repository: &Path,
) -> OracleRun {
    let scratch = scratch_root(package);
    let name = package.rsplit('/').next().expect("oracle package name");
    let executable = scratch.join(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    });
    let tag_list = tags.join(",");
    let mut build = go_command(repository, build_cache);
    build
        .args(["build", "-tags", &tag_list, "-o"])
        .arg(&executable)
        .arg(format!("./rust-tests/parity/oracle/{package}"));
    let build_run = run_bounded(
        build,
        None,
        &scratch.join("build.stdout"),
        &scratch.join("build.stderr"),
        ORACLE_BUILD_TIMEOUT,
    )
    .expect("Go must be available for the committed oracle");
    assert!(
        build_run.status.success(),
        "Go oracle build failed: {}",
        String::from_utf8_lossy(&build_run.stderr)
    );

    let invocation = INVOCATION.fetch_add(1, Ordering::Relaxed);
    let mut command = Command::new(executable);
    command.current_dir(repository);
    run_bounded(
        command,
        None,
        &scratch.join(format!("run-{invocation}.stdout")),
        &scratch.join(format!("run-{invocation}.stderr")),
        ORACLE_TIMEOUT,
    )
    .expect("Go oracle execution must complete within its bounded timeout")
}

/// Runs an already-configured command with a deadline, capturing its output to
/// files. Public so tests that invoke `go test` share this bounded path.
pub fn run_bounded(
    mut command: Command,
    stdin: Option<&[u8]>,
    stdout_path: &Path,
    stderr_path: &Path,
    timeout: Duration,
) -> std::io::Result<OracleRun> {
    command
        .stdout(Stdio::from(std::fs::File::create(stdout_path)?))
        .stderr(Stdio::from(std::fs::File::create(stderr_path)?));
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn()?;
    if let Some(request) = stdin {
        // The oracle payloads here are small and well inside the pipe buffer,
        // so writing before the wait loop cannot block on a full pipe.
        child
            .stdin
            .as_mut()
            .expect("oracle stdin")
            .write_all(request)?;
    }
    drop(child.stdin.take());

    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "oracle subprocess exceeded its bounded timeout",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    Ok(OracleRun {
        status,
        stdout: std::fs::read(stdout_path)?,
        stderr: std::fs::read(stderr_path)?,
    })
}
