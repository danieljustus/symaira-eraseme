//! Synthetic native child fixture; never implements an MCP or Go oracle.
use std::io::Write;
use std::time::Duration;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) == Some("--flood") {
        assert_eq!(args.len(), 2);
        let raw = vec![b'x'; 2 * 1024 * 1024];
        match args[1].as_str() {
            "stdout" => {
                let _ = std::io::stdout().write_all(&raw);
            }
            "stderr" => {
                let _ = std::io::stderr().write_all(&raw);
            }
            _ => panic!("unknown synthetic flood stream"),
        }
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    // Production agent callers pass CLI arguments; they remain fixture input.
    assert!(!args.iter().any(|arg| arg == "--oracle"));
    std::io::stderr()
        .write_all(b"before\xf0\x80\x80after\xffend")
        .unwrap();
    std::process::exit(23);
}
