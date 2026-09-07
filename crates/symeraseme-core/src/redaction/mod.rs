//! Byte-preserving PII redaction and consent-bound review primitives.
mod path;
mod pii;
mod review;

pub use path::{WorkspaceRoot, WorkspaceRootError, read_workspace_file, read_workspace_text};
pub use pii::{
    Address, MAX_PROFILE_LITERAL_BYTES, MAX_PROFILE_VECTOR_ENTRIES, Match, RedactionError,
    RedactionProfile, Rule, collect_matches, redact_bytes, redact_text, rules,
};
pub use review::{Action, Decision, ReviewError, ReviewResult, review_bytes, review_text};

#[cfg(test)]
mod tests {
    use super::path::MAX_FILE_BYTES;
    use super::*;
    use std::fs;
    use std::path::Path;

    #[derive(serde::Deserialize)]
    struct Golden {
        name: String,
        input: String,
        expected: String,
    }

    #[test]
    fn shared_golden_corpus_is_byte_exact() {
        let raw = include_bytes!("../../../../internal/redaction/testdata/golden.json");
        let cases: Vec<Golden> = serde_json::from_slice(raw).expect("golden JSON");
        assert_eq!(cases.len(), 9);
        for case in cases {
            assert_eq!(
                redact_text(&case.input, None).unwrap(),
                case.expected,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn profile_literals_are_case_insensitive_and_overlap_is_deterministic() {
        let profile = RedactionProfile {
            full_name: "Jane Doe".into(),
            name_variants: vec![],
            email_addresses: vec!["jane.doe@example.com".into()],
            phone_numbers: vec!["555-123-4567".into()],
            addresses: vec![Address {
                street: "Main Street 5".into(),
                city: "Berlin".into(),
                postal_code: "10115".into(),
            }],
        };
        let input = "jane.doe@EXAMPLE.com / JANE DOE / 555-123-4567 / MAIN STREET 5";
        assert_eq!(
            redact_text(input, Some(&profile)).unwrap(),
            "[REDACTED-EMAIL] / [REDACTED-NAME] / [REDACTED-PHONE] / [REDACTED-STREET]"
        );
        let unicode = RedactionProfile {
            full_name: "Jürgen Müller".into(),
            ..RedactionProfile::default()
        };
        assert_eq!(
            redact_text("JÜrgen MÜLLER", Some(&unicode)).unwrap(),
            "[REDACTED-NAME]"
        );
        let mut mixed = vec![0xff, b' '];
        mixed.extend_from_slice("JÜrgen MÜLLER".as_bytes());
        mixed.extend_from_slice(&[b' ', 0xfe]);
        assert_eq!(
            redact_bytes(&mixed, Some(&unicode)).unwrap(),
            [vec![0xff, b' '], b"[REDACTED-NAME] ".to_vec(), vec![0xfe]].concat()
        );
    }

    #[test]
    fn arbitrary_bytes_are_preserved_outside_ascii_matches() {
        let input = [
            0xff, 0x00, b' ', b'j', b'a', b'n', b'e', b'@', b'e', b'x', b'a', b'm', b'p', b'l',
            b'e', b'.', b'c', b'o', b'm', 0xfe,
        ];
        let output = redact_bytes(&input, None).unwrap();
        assert_eq!(&output[..3], &[0xff, 0x00, b' ']);
        assert_eq!(&output[output.len() - 1..], &[0xfe]);
        assert!(
            output
                .windows(b"j**e@e*.com".len())
                .any(|w| w == b"j**e@e*.com")
        );
    }

    #[test]
    fn review_enforces_consent_and_quit_is_partial() {
        let input = b"jane@example.com 555-123-4567 alice@example.com";
        let matches = collect_matches(input, None).unwrap();
        let decisions = [Action::Redact, Action::Quit];
        let mut index = 0;
        let result = review_bytes(
            input,
            &matches,
            Some(&mut |_: &Match| {
                let action = decisions[index];
                index += 1;
                action
            }),
        )
        .unwrap();
        assert!(result.quit);
        assert_eq!(result.redacted, 1);
        assert_eq!(result.output, b"j**e@e*.com 555-123-4567 alice@example.com");
    }

    #[test]
    fn safe_root_read_rejects_symlinks_and_traversal() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("ok.txt"), b"jane@example.com").unwrap();
        let root = WorkspaceRoot::open(temp.path()).unwrap();
        assert_eq!(root.read("ok.txt").unwrap(), b"jane@example.com");
        assert!(root.read("../outside.txt").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(temp.path().join("ok.txt"), temp.path().join("link.txt"))
                .unwrap();
            assert!(root.read("link.txt").is_err());
        }
    }

    #[test]
    fn long_domain_email_matches_go_rule_bounds() {
        let domain = std::iter::repeat_n("a", 127).collect::<Vec<_>>().join(".");
        let input = format!("x@{domain}");
        let output = redact_text(&input, None).unwrap();
        assert_eq!(output, format!("x@a*.{}", domain[2..].to_owned()));
    }

    #[test]
    fn adjacent_passport_keyword_preserves_non_pii_bytes() {
        assert_eq!(
            redact_text("passportABC123", None).unwrap(),
            "passport****23"
        );
    }

    #[test]
    fn review_rejects_invalid_matches() {
        let value = b"abc";
        let invalid = Match::new("test", 0, 2, b"ac".to_vec(), b"x".to_vec());
        assert!(matches!(
            review_bytes(value, &[invalid], None::<fn(&Match) -> Action>),
            Err(ReviewError::InvalidMatch(0))
        ));
    }

    #[test]
    fn safe_root_read_rejects_directories_and_oversize_files() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("directory")).unwrap();
        let root = WorkspaceRoot::open(temp.path()).unwrap();
        assert!(matches!(
            root.read("directory"),
            Err(WorkspaceRootError::NotRegularFile)
        ));
        let oversized = temp.path().join("oversized");
        let file = fs::File::create(&oversized).unwrap();
        file.set_len(MAX_FILE_BYTES + 1).unwrap();
        assert!(matches!(
            root.read("oversized"),
            Err(WorkspaceRootError::FileTooLarge)
        ));
    }

    #[test]
    fn profile_limits_are_checked_before_matching() {
        let profile = RedactionProfile {
            full_name: "x".repeat(MAX_PROFILE_LITERAL_BYTES + 1),
            ..RedactionProfile::default()
        };
        assert!(matches!(
            collect_matches(b"x", Some(&profile)),
            Err(RedactionError::InvalidProfileLiteral)
        ));
        let profile = RedactionProfile {
            name_variants: vec!["x".to_owned(); MAX_PROFILE_VECTOR_ENTRIES + 1],
            ..RedactionProfile::default()
        };
        assert!(matches!(
            collect_matches(b"x", Some(&profile)),
            Err(RedactionError::InvalidProfileLiteral)
        ));
    }

    #[test]
    fn forged_empty_match_is_rejected_without_zero_length_windows() {
        let item = Match::new("test", 0, 0, Vec::new(), b"x".to_vec());
        assert!(matches!(
            review_bytes(b"abc", &[item], None::<fn(&Match) -> Action>),
            Err(ReviewError::InvalidMatch(0))
        ));
    }

    #[test]
    fn absolute_inside_path_is_confined_to_the_open_root() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("inside.txt");
        fs::write(&path, b"jane@example.com").unwrap();
        assert_eq!(
            read_workspace_file(&path, Some(temp.path())).unwrap(),
            b"jane@example.com"
        );
        let outside = temp.path().parent().unwrap().join("sibling-nope.txt");
        assert!(read_workspace_file(&outside, Some(temp.path())).is_err());
    }

    #[test]
    fn no_content_is_included_in_path_errors() {
        let temp = tempfile::tempdir().unwrap();
        let error = read_workspace_file(Path::new("missing"), Some(temp.path())).unwrap_err();
        assert!(!error.to_string().contains("jane"));
    }
}
