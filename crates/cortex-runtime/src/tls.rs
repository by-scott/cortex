use std::io;
use std::path::Path;
use std::sync::Arc;

/// Build a rustls `ServerConfig` from PEM cert and key files.
///
/// # Errors
/// Returns `io::Error` if the files cannot be read or parsed.
pub fn build_server_config(
    cert_path: &Path,
    key_path: &Path,
) -> io::Result<Arc<rustls::ServerConfig>> {
    let cert_data = std::fs::read(cert_path)?;
    let key_data = std::fs::read(key_path)?;

    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut cert_data.as_slice())
            .filter_map(Result::ok)
            .collect();
    if certs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "no valid certificates found in cert file",
        ));
    }

    let key = rustls_pemfile::private_key(&mut key_data.as_slice())?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no private key found"))?;

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    Ok(Arc::new(config))
}
