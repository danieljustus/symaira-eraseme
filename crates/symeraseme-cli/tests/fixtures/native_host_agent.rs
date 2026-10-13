//! Synthetic blocking agent for the native Windows HTTP replay.
//!
//! It is not a Go oracle and has no host configuration or credentials. Like the
//! original helper, it publishes its PID by writing a temporary file and renaming
//! it to AGENT_STARTED, then blocks for 30 seconds.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

fn main() {
    let started = std::env::var_os("AGENT_STARTED").expect("AGENT_STARTED is required");
    let started = Path::new(&started);
    // Rename a completed file so the reader never observes a partial PID.
    let temporary = started.with_file_name("pid.tmp");
    // create_new refuses a stale or planted temporary file instead of following it.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .expect("create private pid temporary");
    writeln!(file, "{}", std::process::id()).expect("write pid");
    file.sync_all().expect("sync pid");
    drop(file);
    std::fs::rename(&temporary, started).expect("publish pid");
    std::thread::sleep(Duration::from_secs(30));
}
