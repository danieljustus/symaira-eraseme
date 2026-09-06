//! Trusted confirmation-link extraction.
//!
//! This module is intentionally only the synchronous, side-effect-free part
//! of confirmation handling. It does not click links or start a browser.

/// The fixed broker host set used by the Go confirmation oracle.
pub const KNOWN_BROKER_DOMAINS: &[&str] = &[
    "acxiom.com",
    "oracle.com",
    "schufa.de",
    "beenverified.com",
    "spokeo.com",
    "intelius.com",
    "whitepages.com",
    "mylife.com",
    "peekyou.com",
    "pipl.com",
    "radaris.com",
    "truepeoplesearch.com",
    "ussearch.com",
    "peoplefinders.com",
    "instantcheckmate.com",
    "truthfinder.com",
    "addresses.com",
    "anywho.com",
    "dexknows.com",
    "meridiandata.us",
    "experian.com",
    "transunion.com",
    "equifax.com",
];

/// Extract unique HTTPS links to known broker hosts.
///
/// Candidate recognition matches the Go `https?://[^\\s<>"']+` expression.
/// Links are returned in source-stable order for equal scores, with the
/// shortest parsed path first and the shortest original URL as the tie-breaker.
pub fn extract_confirmation_links(text: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut seen = Vec::new();
    let mut cursor = 0;

    while let Some(relative_start) = text[cursor..].find("http") {
        let start = cursor + relative_start;
        let scheme_len = if text[start..].starts_with("https://") {
            8
        } else if text[start..].starts_with("http://") {
            7
        } else {
            cursor = start + "http".len();
            continue;
        };
        let candidate_end = text[start + scheme_len..]
            .char_indices()
            .find_map(|(offset, character)| {
                (character.is_whitespace() || "<>\"'".contains(character))
                    .then_some(start + scheme_len + offset)
            })
            .unwrap_or(text.len());
        let candidate = &text[start..candidate_end];
        let candidate = candidate.trim_end_matches(|character| ".,;:!?)]>".contains(character));

        if !seen.iter().any(|previous| previous == candidate)
            && parse_allowed_url(candidate).is_some()
        {
            seen.push(candidate.to_owned());
            links.push(candidate.to_owned());
        }
        cursor = candidate_end.max(start + scheme_len);
    }

    links.sort_by(|left, right| {
        let left_url = parse_allowed_url(left).expect("validated confirmation URL");
        let right_url = parse_allowed_url(right).expect("validated confirmation URL");
        left_url
            .path_len
            .cmp(&right_url.path_len)
            .then_with(|| left.len().cmp(&right.len()))
    });
    links
}

#[derive(Clone, Copy)]
struct ParsedUrl {
    path_len: usize,
}

fn parse_allowed_url(candidate: &str) -> Option<ParsedUrl> {
    if !candidate.starts_with("https://")
        || !has_valid_percent_escapes(candidate)
        || candidate.chars().any(char::is_control)
    {
        return None;
    }

    let scheme_end = "https://".len();
    let authority_end = scheme_end
        + candidate[scheme_end..]
            .find(['/', '?', '#'])
            .unwrap_or(candidate[scheme_end..].len());
    let authority = &candidate[scheme_end..authority_end];
    if authority.is_empty() {
        return None;
    }
    let hostport = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if hostport.starts_with('[') {
        return None;
    }
    let (host, port) = match hostport.split_once(':') {
        Some((host, port)) if !hostport[host.len() + 1..].contains(':') => (host, Some(port)),
        Some(_) => return None,
        None => (hostport, None),
    };
    if host.is_empty()
        || port
            .is_some_and(|port| !port.is_empty() && !port.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }

    let host = host.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    if !KNOWN_BROKER_DOMAINS.contains(&host) {
        return None;
    }

    let path_end = candidate[authority_end..]
        .find(['?', '#'])
        .map_or(candidate.len(), |offset| authority_end + offset);
    Some(ParsedUrl {
        path_len: decoded_path_len(&candidate[authority_end..path_end]),
    })
}

fn decoded_path_len(path: &str) -> usize {
    let bytes = path.as_bytes();
    let mut length = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            length += 1;
            index += 3;
        } else {
            length += 1;
            index += 1;
        }
    }
    length
}

fn has_valid_percent_escapes(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.iter().enumerate().all(|(index, byte)| {
        *byte != b'%'
            || (index + 2 < bytes.len()
                && bytes[index + 1].is_ascii_hexdigit()
                && bytes[index + 2].is_ascii_hexdigit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn filters_deduplicates_and_sorts_like_go() {
        let links = extract_confirmation_links(
            "https://evil.example/confirm http://acxiom.com/insecure \
             https://www.acxiom.com/long-confirm https://acxiom.com/confirm. \
             https://acxiom.com/confirm",
        );
        assert_eq!(
            links,
            [
                "https://acxiom.com/confirm",
                "https://www.acxiom.com/long-confirm"
            ]
        );
    }

    #[test]
    fn rejects_hostile_and_malformed_candidates() {
        let text = [
            "http://acxiom.com/confirm",
            "https://evil.example/confirm",
            "https://acxiom.com.evil.example/confirm",
            "https://acxiom.com@evil.example/confirm",
            "https://acxiom.com/%zz",
            "https:///confirm",
            "javascript://acxiom.com/confirm",
        ]
        .join(" ");

        assert!(extract_confirmation_links(&text).is_empty());
    }

    #[test]
    fn allows_only_one_www_normalization_and_preserves_original_url() {
        assert_eq!(
            extract_confirmation_links("https://WWW.ACXIOM.COM/confirm"),
            ["https://WWW.ACXIOM.COM/confirm"]
        );
        assert!(extract_confirmation_links("https://www.www.acxiom.com/confirm").is_empty());
    }

    proptest! {
        #[test]
        fn hostile_domains_never_pass_allowlist(
            label in "[a-z]{1,20}",
            suffix in "[a-z]{1,12}",
        ) {
            let text = format!("https://{label}.{suffix}.invalid/confirm");
            prop_assert!(extract_confirmation_links(&text).is_empty());
        }
    }
}
