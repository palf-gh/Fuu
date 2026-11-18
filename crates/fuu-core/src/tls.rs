//! TLS client configuration shared by IMAP and SMTP.
//!
//! Certificates are verified with the OS trust store first (so corporate CAs and
//! user policy apply). If, and only if, that fails because the issuer is unknown,
//! the chain is verified again against the Mozilla root store (webpki-roots).
//!
//! Why the fallback: on Windows the platform verifier builds chains with
//! `CERT_CHAIN_CACHE_ONLY_URL_RETRIEVAL`, so roots that Windows has not yet
//! downloaded through its automatic root update (for example GlobalSign Root R46,
//! used by lolipop.jp) are reported as unknown even though they are trusted
//! publicly. Any other verification failure (expiry, wrong name, revoked, bad
//! signature) is never retried and stays a failure.

use std::sync::{Arc, OnceLock};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, Error, RootCertStore, SignatureScheme};

#[derive(Debug)]
struct PlatformThenWebPki {
    platform: Arc<dyn ServerCertVerifier>,
    webpki: Arc<WebPkiServerVerifier>,
}

impl ServerCertVerifier for PlatformThenWebPki {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        match self.platform.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now) {
            Err(Error::InvalidCertificate(CertificateError::UnknownIssuer)) => {
                self.webpki.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
            }
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.platform.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.platform.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.platform.supported_verify_schemes()
    }
}

fn build() -> Result<Arc<rustls::ClientConfig>, String> {
    let provider: Arc<CryptoProvider> = Arc::new(rustls::crypto::ring::default_provider());
    let platform = rustls_platform_verifier::Verifier::new(provider.clone()).map_err(|e| e.to_string())?;
    let roots = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let webpki = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .build()
        .map_err(|e| e.to_string())?;
    let verifier = PlatformThenWebPki { platform: Arc::new(platform), webpki };
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .dangerous()
        // Not a bypass: a verifier that always verifies, with a fallback trust store.
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// The shared client configuration (built once).
pub fn client_config() -> Result<Arc<rustls::ClientConfig>, String> {
    static CONFIG: OnceLock<Result<Arc<rustls::ClientConfig>, String>> = OnceLock::new();
    CONFIG.get_or_init(build).clone()
}

#[cfg(test)]
mod tests {
    #[test]
    fn mozilla_roots_include_globalsign_r46() {
        let found = webpki_roots::TLS_SERVER_ROOTS.iter().any(|ta| {
            let subject: &[u8] = ta.subject.as_ref();
            subject.windows(b"GlobalSign Root R46".len()).any(|w| w == b"GlobalSign Root R46")
        });
        assert!(found, "webpki-roots must carry GlobalSign Root R46");
    }

    #[test]
    fn builds_a_client_config() {
        super::client_config().expect("TLS configuration");
    }
}
