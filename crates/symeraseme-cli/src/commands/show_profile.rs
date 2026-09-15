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
        "json" => {
            let mut output = serde_json::to_vec(&profile)
                .map_err(|_| "identity: profile output failed".to_owned())?;
            output.push(b'\n');
            Ok(output)
        }
        _ => Err(format!(
            "invalid output format \"{output_format}\": use text or json"
        )),
    }
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
