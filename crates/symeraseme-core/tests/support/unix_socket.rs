//! Bind relative socket paths in an isolated child, never change parent CWD.
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixListener;
use std::path::{Component, Path};
use std::process::Command;

const RELATIVE_PATH: &str = "SYMERASEME_TEST_SOCKET_RELATIVE_PATH";

pub fn create(directory: &Path, relative: &str) {
    let path = Path::new(relative);
    assert!(!relative.is_empty());
    assert!(
        path.components()
            .all(|part| matches!(part, Component::Normal(_)))
    );
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "unix_socket::child", "--ignored"])
        .current_dir(directory)
        .env(RELATIVE_PATH, relative)
        .output()
        .unwrap();
    assert!(output.status.success(), "socket fixture child: {output:?}");
    assert!(
        std::fs::symlink_metadata(directory.join(path))
            .unwrap()
            .file_type()
            .is_socket()
    );
}

#[test]
#[ignore = "only invoked by its parent in an owned fixture directory"]
fn child() {
    let relative = std::env::var_os(RELATIVE_PATH).expect("parent supplies the fixture pathname");
    // Closing a Unix listener leaves its real socket entry until TempDir cleanup.
    let _listener = UnixListener::bind(relative).unwrap();
}

#[test]
fn relative_fixture_keeps_real_socket_inside_long_parent() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("x".repeat(128));
    std::fs::create_dir(&directory).unwrap();
    create(&directory, "socket");
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
}
