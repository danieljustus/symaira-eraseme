//! Shared platform roots for mailbox TLS and OAuth2 HTTPS.
use rustls::RootCertStore;
use rustls::pki_types::CertificateDer;

pub(super) fn platform_certificates() -> Result<Vec<CertificateDer<'static>>, String> {
    let loaded = rustls_native_certs::load_native_certs();
    let mut certificates = loaded.certs;
    #[cfg(windows)]
    add_windows_machine_roots(&mut certificates)?;
    let mut parsed = RootCertStore::empty();
    certificates.retain(|cert| parsed.add(cert.clone()).is_ok());
    if certificates.is_empty() {
        return Err(format!(
            "no platform TLS root certificates could be loaded: {:?}",
            loaded.errors
        ));
    }
    Ok(certificates)
}

#[cfg(windows)]
fn add_windows_machine_roots(
    certificates: &mut Vec<CertificateDer<'static>>,
) -> Result<(), String> {
    let custom_roots_configured = std::env::var_os("SSL_CERT_FILE").is_some()
        || std::env::var_os("SSL_CERT_DIR").is_some_and(|dirs| {
            std::env::split_paths(&dirs).any(|path| !path.as_os_str().is_empty())
        });
    if custom_roots_configured {
        return Ok(());
    }

    use schannel::cert_context::ValidUses;
    use schannel::cert_store::CertStore;

    let store = CertStore::open_local_machine("ROOT").map_err(|error| {
        format!("failed to load Windows LocalMachine ROOT certificates: {error}")
    })?;
    for cert in store.certs() {
        let server_auth = cert.valid_uses().is_ok_and(|uses| match uses {
            ValidUses::All => true,
            ValidUses::Oids(oids) => oids.iter().any(|oid| oid == "1.3.6.1.5.5.7.3.1"),
        });
        if server_auth && cert.is_time_valid().unwrap_or(false) {
            certificates.push(cert.to_der().to_vec().into());
        }
    }
    Ok(())
}
