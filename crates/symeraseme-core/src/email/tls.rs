//! Shared platform roots for mailbox TLS and OAuth2 HTTPS.
use rustls::RootCertStore;
use rustls::pki_types::CertificateDer;

pub(super) fn platform_certificates() -> Result<Vec<CertificateDer<'static>>, String> {
    let loaded = rustls_native_certs::load_native_certs();
    let mut certificates = loaded.certs;
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
