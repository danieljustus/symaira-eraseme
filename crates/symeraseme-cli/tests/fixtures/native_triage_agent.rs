//! Synthetic child for private native triage process comparisons.
//! It records invocation counts and implements no Go CLI or MCP oracle.
use std::io::Write;

fn main() {
    if let Some(path) =
        std::env::var_os("SYMERASEME_NATIVE_AGENT_CONTROL").filter(|value| !value.is_empty())
    {
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = options
            .open(path)
            .and_then(|mut file| file.write_all(b"invoked\n"));
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
    if std::env::var_os("AGENT_STDERR_ESCAPED").is_some_and(|value| !value.is_empty()) {
        std::io::stderr().write_all(b"before\xffafter").unwrap();
        std::process::exit(23);
    }
    let args = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let response = if args.contains("--model oracle-env-model") {
        r#"{"classification":"confirmed","confidence":0.93,"summary":"model selected from environment","extracted_fields":{"ticket":"T-42"}}"#
    } else if args.contains("--model oracle-flag-model") {
        r#"{"classification":"confirmed","confidence":0.93,"summary":"model selected from flag","extracted_fields":{"ticket":"T-42"}}"#
    } else if args.contains("rejection classifier") {
        r#"{"classification":"address_mismatch","confidence":0.91,"summary":"address differs","key_points":[],"jurisdiction":"GDPR"}"#
    } else if args.contains("email classifier") {
        r#"{"classification":"confirmed","confidence":0.93,"summary":"deletion confirmed","extracted_fields":{"ticket":"T-42"}}"#
    } else {
        r#"{"classification":"other","confidence":0.1,"summary":"unexpected prompt","key_points":[],"jurisdiction":"unknown"}"#
    };
    println!("{response}");
}
