#![no_main]

use libfuzzer_sys::fuzz_target;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const MAX_INPUT: usize = 64 * 1024;
const MAX_CAPTURE_PER_STREAM: usize = 32 * 1024;
const CHILD_TIMEOUT: Duration = Duration::from_millis(500);
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct TempDataDir(PathBuf);

impl Drop for TempDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn drain<R: Read + Send + 'static>(mut stream: R) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut captured = Vec::with_capacity(MAX_CAPTURE_PER_STREAM);
        let mut buffer = [0; 4096];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let remaining = MAX_CAPTURE_PER_STREAM.saturating_sub(captured.len());
                    captured.extend_from_slice(&buffer[..read.min(remaining)]);
                }
            }
        }
        captured
    })
}

fn reap(child: &mut Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + CHILD_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("MCP stdio child exceeded {CHILD_TIMEOUT:?}");
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("could not wait for MCP stdio child: {error}");
            }
        }
    }
}

fuzz_target!(|input: &[u8]| {
    let binary = std::env::var_os("SYMERASEME_RUST_BIN")
        .expect("set SYMERASEME_RUST_BIN to the built symeraseme-rust executable");
    if input.len() > MAX_INPUT {
        return;
    }

    let data_dir = TempDataDir(std::env::temp_dir().join(format!(
        "symeraseme-mcp-fuzz-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    )));
    std::fs::create_dir(&data_dir.0).expect("create isolated MCP fuzz data directory");
    let mut child = Command::new(binary)
        .args(["mcp", "--stdio"])
        .current_dir(&data_dir.0)
        .env("HOME", &data_dir.0)
        .env("SYMERASEME_DATA_DIR", &data_dir.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start symeraseme-rust mcp --stdio");

    let stdout = drain(child.stdout.take().expect("piped stdout"));
    let stderr = drain(child.stderr.take().expect("piped stderr"));
    let mut stdin = child.stdin.take().expect("piped stdin");
    let input = input.to_vec();
    let writer = thread::spawn(move || {
        let _ = stdin.write_all(&input);
        // Dropping stdin signals EOF to the stdio server.
    });

    let status = reap(&mut child);
    writer.join().expect("stdin writer thread");
    let _ = stdout.join().expect("stdout reader thread");
    let _ = stderr.join().expect("stderr reader thread");
    assert!(
        status.code().is_some(),
        "MCP stdio child terminated by signal"
    );
});
