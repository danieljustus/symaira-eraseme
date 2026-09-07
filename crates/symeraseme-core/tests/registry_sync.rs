use std::io::{Cursor, Read, Write};

use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use symeraseme_core::registry::{SyncResponse, SyncTransport, sync_with_transport};
use tar::{Builder, Header};

struct StaticTransport {
    status: u16,
    body: Vec<u8>,
}

impl SyncTransport for StaticTransport {
    fn get(
        &self,
        _url: &str,
        _timeout: std::time::Duration,
    ) -> Result<SyncResponse, symeraseme_core::registry::RegistryError> {
        Ok(SyncResponse {
            status: self.status,
            body: Box::new(Cursor::new(self.body.clone())),
        })
    }
}

fn archive(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut compressed = GzEncoder::new(Vec::new(), Compression::default());
    {
        let mut tar = Builder::new(&mut compressed);
        for (name, body, kind) in entries {
            let mut header = Header::new_gnu();
            if name.contains("../") {
                let raw = header.as_mut_bytes();
                raw[..100].fill(0);
                raw[..name.len()].copy_from_slice(name.as_bytes());
            } else {
                header.set_path(name).unwrap();
            }
            header.set_mode(*kind);
            if *kind == 0o700 {
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
            } else {
                header.set_entry_type(tar::EntryType::Regular);
                header.set_size(body.len() as u64);
            }
            header.set_cksum();
            tar.append(&header, *body).unwrap();
        }
        tar.finish().unwrap();
    }
    compressed.finish().unwrap()
}

fn valid_archive() -> Vec<u8> {
    let broker = b"id: test\nname: Test\nwebsite: https://example.test\ncategory: other\njurisdictions: [US]\nlaws: [GDPR]\npriority: low\nopt_out:\n  - type: email\n    endpoint: a@example.test\n";
    archive(&[
        (
            "manifest.json",
            br#"{"schema_version":1,"schemas":{"broker":"schemas/broker.schema.json"}}"#,
            0o600,
        ),
        (
            "schemas/broker.schema.json",
            br#"{"schema_version":1}"#,
            0o600,
        ),
        ("schemas/safe..name.txt", b"allowed", 0o600),
        ("brokers/us/test.yaml", broker, 0o600),
    ])
}

#[test]
fn sync_validates_then_replaces_existing_registry() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("registry");
    std::fs::create_dir_all(destination.join("old")).unwrap();
    std::fs::write(destination.join("old/state"), b"old-bytes").unwrap();
    let transport = StaticTransport {
        status: 200,
        body: valid_archive(),
    };

    sync_with_transport("http://127.0.0.1/test", &destination, &transport).unwrap();
    let installed = std::fs::read(destination.join("brokers/us/test.yaml")).unwrap();
    assert_eq!(&installed[..3], b"id:");
    assert_eq!(
        std::fs::read(destination.join("schemas/safe..name.txt")).unwrap(),
        b"allowed"
    );
    assert!(!destination.join("old/state").exists());
}

#[test]
fn sync_rejects_traversal_and_preserves_old_bytes() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("registry");
    std::fs::create_dir_all(&destination).unwrap();
    let old = destination.join("sentinel");
    std::fs::write(&old, b"old-bytes").unwrap();
    let body = archive(&[("../escape", b"escaped", 0o600)]);
    let transport = StaticTransport { status: 200, body };

    assert!(sync_with_transport("http://127.0.0.1/test", &destination, &transport).is_err());
    assert_eq!(std::fs::read(&old).unwrap(), b"old-bytes");
    assert!(!root.path().join("escape").exists());
}

#[test]
fn sync_rejects_non_loopback_http_before_transport() {
    let transport = StaticTransport {
        status: 200,
        body: Vec::new(),
    };
    let root = tempfile::tempdir().unwrap();
    assert!(
        sync_with_transport(
            "http://example.test/registry.tar.gz",
            root.path().join("registry"),
            &transport
        )
        .is_err()
    );
}

#[test]
fn sync_rejects_noncanonical_gzip_and_tar_trailing_data() {
    let mut corrupt_trailer = valid_archive();
    *corrupt_trailer.last_mut().unwrap() ^= 1;
    let mut raw_trailing = valid_archive();
    raw_trailing.extend_from_slice(b"raw trailing");
    let mut concatenated = valid_archive();
    concatenated.extend_from_slice(&valid_archive());

    let mut tar_bytes = Vec::new();
    GzDecoder::new(Cursor::new(valid_archive()))
        .read_to_end(&mut tar_bytes)
        .unwrap();
    let mut payload = GzEncoder::new(Vec::new(), Compression::default());
    payload.write_all(&tar_bytes).unwrap();
    payload.write_all(b"decompressed trailing").unwrap();
    let decompressed_trailing = payload.finish().unwrap();
    let mut zero_payload = GzEncoder::new(Vec::new(), Compression::default());
    zero_payload.write_all(&tar_bytes).unwrap();
    zero_payload.write_all(&[0]).unwrap();
    let zero_decompressed_trailing = zero_payload.finish().unwrap();

    for body in [
        corrupt_trailer,
        raw_trailing,
        concatenated,
        decompressed_trailing,
        zero_decompressed_trailing,
    ] {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("registry");
        std::fs::create_dir_all(&destination).unwrap();
        let sentinel = destination.join("sentinel");
        std::fs::write(&sentinel, b"old").unwrap();
        let transport = StaticTransport { status: 200, body };
        assert!(sync_with_transport("http://127.0.0.1/test", &destination, &transport).is_err());
        assert_eq!(std::fs::read(sentinel).unwrap(), b"old");
    }
}
