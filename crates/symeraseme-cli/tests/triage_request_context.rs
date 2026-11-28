//! `classify-reply` and `generate-rebuttal` use the identity profile and the
//! request's broker (#1359). The Go adapters passed neither, so the user's
//! name reached the model unredacted and a rebuttal was signed `<no value>`.
//! A fake Claude Code on PATH records each prompt and answers with fixed JSON;
//! no model is called.

use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use symeraseme_core::storage::Store;

const NAME: &str = "Erika Mustermann";
// Dummy key: keeps every master-key lookup away from the real keychain.
const KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_symeraseme-rust"))
        .args(args)
        .env_clear()
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("SYMERASEME_DATA_DIR", root.join("data"))
        .env("SYMERASEME_IDENTITY_MASTER_KEY", KEY)
        .env("SYMERASEME_LLM_PROVIDER", "agent")
        .env("SYMERASEME_AGENT_BACKEND", "claude")
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", root.join("bin").display()),
        )
        .env("AGENT_CAPTURE", root.join("prompts"))
        .output()
        .expect("run symeraseme")
}

fn ok(root: &Path, args: &[&str]) -> String {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 stdout")
}

/// The prompts the fake agent received, in call order.
fn prompts(root: &Path) -> Vec<String> {
    let bytes = fs::read(root.join("prompts")).expect("agent was called");
    bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8(part.to_vec()).unwrap())
        .collect()
}

#[test]
fn reply_triage_uses_the_profile_and_the_request_broker() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::create_dir_all(root.join("home")).unwrap();
    let agent = root.join("bin/claude");
    fs::write(
        &agent,
        "#!/bin/sh\nprintf '%s\\0' \"$2\" >> \"$AGENT_CAPTURE\"\nprintf '%s\\n' '{\"classification\":\"identity_challenged\",\"confidence\":0.9,\"summary\":\"asks for identity\",\"extracted_fields\":{},\"key_points\":[],\"jurisdiction\":\"GDPR\"}'\n",
    )
    .unwrap();
    fs::set_permissions(&agent, fs::Permissions::from_mode(0o700)).unwrap();

    ok(
        root,
        &[
            "init-profile",
            "--full-name",
            NAME,
            "--email",
            "erika@example.invalid",
        ],
    );
    ok(
        root,
        &[
            "plan",
            "create",
            "--campaign",
            "c",
            "--jurisdiction",
            "DE",
            "--max",
            "1",
        ],
    );
    let store = Store::open(root.join("data/symeraseme.db")).unwrap();
    let broker_id: String = store
        .connection()
        .query_row(
            "SELECT broker_id FROM removal_requests WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    store
        .connection()
        .execute(
            "INSERT INTO inbox_replies (request_id, message_id, subject, snippet)
             VALUES (1, '<reply@example.invalid>', 'Re: Data Deletion Request', ?1)",
            [format!("Dear {NAME}, please send a copy of your passport.")],
        )
        .unwrap();
    drop(store);
    let broker = symeraseme_core::registry::load_embedded()
        .unwrap()
        .into_iter()
        .find(|broker| broker.id == broker_id)
        .expect("planned broker is in the registry");

    ok(root, &["classify-reply", "1", "--save=false"]);
    let rebuttal: Value = serde_json::from_str(&ok(
        root,
        &["--output", "json", "generate-rebuttal", "1", "--save=false"],
    ))
    .unwrap();

    let prompts = prompts(root);
    assert_eq!(prompts.len(), 2, "{prompts:?}");
    for prompt in &prompts {
        assert!(
            !prompt.contains(NAME),
            "the name reached the model: {prompt}"
        );
    }
    assert!(
        prompts[0].contains(&format!("Broker: {} ({})", broker.name, broker.website)),
        "{}",
        prompts[0]
    );
    assert!(
        prompts[1].contains(&format!("Broker: {}", broker.name)),
        "{}",
        prompts[1]
    );

    let body = rebuttal["rebuttal_body"].as_str().unwrap();
    assert_eq!(rebuttal["llm_used"], true, "{rebuttal}");
    assert!(!body.contains("<no value>"), "{body}");
    assert!(
        body.contains(&format!("Yours faithfully,\n{NAME}")),
        "{body}"
    );
    assert!(
        body.contains("- Email address: erika@example.invalid"),
        "{body}"
    );
}
