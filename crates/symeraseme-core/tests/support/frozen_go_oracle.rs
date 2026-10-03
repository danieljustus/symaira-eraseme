use sha2::{Digest, Sha256};
use std::sync::OnceLock;

pub struct Observation {
    pub case: &'static str,
    pub package: &'static str,
    pub request: &'static [u8],
    pub stdout: &'static [u8],
    pub stderr: &'static [u8],
    pub exit_status: i32,
}

macro_rules! observation {
    ($case:literal, $package:literal, $status:literal) => {
        Observation {
            case: $case,
            package: $package,
            request: include_bytes!(concat!(
                "../../../../tests/fixtures/go-frozen/identity-crypto/",
                $case,
                ".stdin"
            )),
            stdout: include_bytes!(concat!(
                "../../../../tests/fixtures/go-frozen/identity-crypto/",
                $case,
                ".stdout"
            )),
            stderr: include_bytes!(concat!(
                "../../../../tests/fixtures/go-frozen/identity-crypto/",
                $case,
                ".stderr"
            )),
            exit_status: $status,
        }
    };
}

const OBSERVATIONS: &[Observation] = &[
    observation!("identity-canonical", "identity", 0),
    observation!("identity-hash", "identity", 0),
    observation!("generic-float64", "identity", 0),
    observation!("identity-go-encrypt", "identity", 0),
    observation!("identity-go-decrypt", "identity", 0),
    observation!("identity-tamper-rejected", "identity", 1),
    observation!("crypto-go-encrypt", "crypto", 0),
    observation!("crypto-go-decrypt", "crypto", 0),
    observation!("crypto-tamper-rejected", "crypto", 1),
    observation!("identity-rust-writer-go-decrypt", "identity", 0),
    observation!("identity-rust-writer-tamper-rejected", "identity", 1),
    observation!("crypto-rust-writer-go-decrypt", "crypto", 0),
    observation!("crypto-rust-writer-tamper-rejected", "crypto", 1),
    observation!("identity-original-rust-writer-go-decrypt", "identity", 0),
    observation!(
        "identity-original-rust-writer-tamper-rejected",
        "identity",
        1
    ),
];

pub fn live_mode() -> bool {
    match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
        Ok("1") => true,
        Ok("0") | Err(std::env::VarError::NotPresent) => false,
        _ => panic!("SYMERASEME_PARITY_LIVE_GO must be 0 or 1"),
    }
}

fn provenance() -> &'static serde_json::Value {
    static PROVENANCE: OnceLock<serde_json::Value> = OnceLock::new();
    PROVENANCE.get_or_init(|| {
        let provenance: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../../tests/fixtures/go-frozen/identity-crypto/manifest.json"
        ))
        .unwrap();
        assert_eq!(
            provenance["observations"].as_array().unwrap().len(),
            OBSERVATIONS.len()
        );
        assert_eq!(provenance["go_version"], "go version go1.26.6 linux/amd64");
        provenance
    })
}

fn verify(observation: &'static Observation) -> &'static Observation {
    let recorded = provenance()["observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["case"] == observation.case)
        .unwrap();
    assert_eq!(recorded["package"], observation.package);
    assert_eq!(recorded["exit_status"], observation.exit_status);
    for (field, bytes) in [
        ("stdin", observation.request),
        ("stdout", observation.stdout),
        ("stderr", observation.stderr),
    ] {
        assert_eq!(
            recorded[field]["bytes"],
            bytes.len(),
            "{} {field} length",
            observation.case
        );
        let sha256: String = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(
            recorded[field]["sha256"], sha256,
            "{} {field} digest",
            observation.case
        );
    }
    observation
}

pub fn case(name: &str) -> &'static Observation {
    all_cases()
        .find(|observation| observation.case == name)
        .expect("known actual Go observation")
}

pub fn all_cases() -> impl Iterator<Item = &'static Observation> {
    OBSERVATIONS.iter().map(verify)
}

pub fn run(package: &str, request: &[u8]) -> Vec<u8> {
    let observation = verify(
        OBSERVATIONS
            .iter()
            .find(|observation| observation.package == package && observation.request == request)
            .expect("unrecorded Go request: refuse to fabricate an oracle result"),
    );
    assert_eq!(observation.exit_status, 0, "recorded Go operation failed");
    observation.stdout.to_vec()
}
