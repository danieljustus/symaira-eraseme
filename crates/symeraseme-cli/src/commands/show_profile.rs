//! `show-profile` command adapter.

use std::fmt::Write as _;
use std::path::Path;

use symeraseme_core::identity::{
    MasterKeyResolver, Profile, ProfileAddress, ProfilePaths, load_profile,
};
use symeraseme_core::templating::{Address, RenderContext};

pub const COMMAND_GROUP: &str = "show-profile";

/// Load and render the process profile without creating or modifying state.
///
/// The loader and key resolver are the same read-only primitives used by the
/// future CLI profile-management slice. The render context is deliberately
/// built before either output mode so both modes consume the same public data.
pub fn execute(output_format: &str) -> Result<Vec<u8>, String> {
    let paths = ProfilePaths::from_process();
    let mut keys = MasterKeyResolver::from_process();
    if std::env::var_os("SYMERASEME_DISABLE_KEYRING").is_some() {
        keys.disable_keyring();
    }
    let profile =
        load_profile(Path::new(""), &paths, &mut keys).map_err(|error| error.to_string())?;
    let context = render_context(&profile);

    match output_format {
        "text" => Ok(render_text(&context)),
        "json" => render_json(&profile),
        _ => Err(format!(
            "invalid output format \"{output_format}\": use text or json"
        )),
    }
}

fn render_json(profile: &Profile) -> Result<Vec<u8>, String> {
    let encoded =
        serde_json::to_vec(profile).map_err(|_| "identity: profile output failed".to_owned())?;
    let mut output = escape_go_html(&encoded);
    output.push(b'\n');
    Ok(output)
}

fn escape_go_html(encoded: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(encoded.len());
    let mut in_string = false;
    let mut position = 0;
    while let Some(&byte) = encoded.get(position) {
        if byte == b'"' {
            in_string = !in_string;
            output.push(byte);
            position += 1;
            continue;
        }
        if !in_string {
            output.push(byte);
            position += 1;
            continue;
        }
        if byte == b'\\' {
            output.push(byte);
            position += 1;
            if let Some(&escaped) = encoded.get(position) {
                output.push(escaped);
                position += 1;
            }
            continue;
        }
        let replacement = match byte {
            b'<' => Some(br#"\u003c"#),
            b'>' => Some(br#"\u003e"#),
            b'&' => Some(br#"\u0026"#),
            0xe2 if encoded.get(position..position + 3) == Some(&[0xe2, 0x80, 0xa8][..]) => {
                Some(br#"\u2028"#)
            }
            0xe2 if encoded.get(position..position + 3) == Some(&[0xe2, 0x80, 0xa9][..]) => {
                Some(br#"\u2029"#)
            }
            _ => None,
        };
        if let Some(replacement) = replacement {
            output.extend_from_slice(replacement);
            position += if byte == 0xe2 { 3 } else { 1 };
        } else {
            output.push(byte);
            position += 1;
        }
    }
    output
}

fn render_context(profile: &Profile) -> RenderContext {
    RenderContext {
        full_name: profile.full_name.clone(),
        name_variants: profile.name_variants.clone(),
        date_of_birth: profile.date_of_birth.clone(),
        addresses: profile.addresses.iter().map(render_address).collect(),
        email_addresses: profile.email_addresses.clone(),
        phone_numbers: profile.phone_numbers.clone(),
        jurisdictions: profile.jurisdictions.clone(),
        ..RenderContext::default()
    }
}

fn render_address(address: &ProfileAddress) -> Address {
    Address {
        street: address.street.clone(),
        city: address.city.clone(),
        postal_code: address.postal_code.clone(),
        country: address.country.clone(),
    }
}

fn render_text(context: &RenderContext) -> Vec<u8> {
    let mut output = String::new();
    let _ = writeln!(output, "Name: {}", context.full_name);
    for value in &context.email_addresses {
        let _ = writeln!(output, "Email: {value}");
    }
    for value in &context.jurisdictions {
        let _ = writeln!(output, "Jurisdiction: {value}");
    }
    output.into_bytes()
}
