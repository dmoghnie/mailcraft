//! Android's own trust store, asked when the bundled Mozilla roots don't vouch for a server.
//!
//! There's no root list to read on Android (`rustls-native-certs` finds none), so "the system's
//! roots" are asked of the platform instead: its TrustManager, through `rustls-platform-verifier`,
//! which trusts the system's CAs and those the person installed (as the app's network security
//! config allows), less any they turned off. A server is then trusted as on the desktop: by
//! Mozilla's roots or the system's. Only an unknown issuer is asked about again; a certificate
//! that's expired, revoked or for another name stays refused.
//!
//! The platform's side needs the app's JVM and context, which the phone library hands over
//! (`rustls_platform_verifier::android::init_with_env`) before it calls [`platform_ready`]; until
//! then nothing is asked (the crate would panic asked before), and Mozilla's roots decide alone.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use rustls::client::danger::{ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, Error};

static READY: AtomicBool = AtomicBool::new(false);

/// The platform's side is set up: from now on an unknown issuer is asked of Android's trust
/// store. Called by the phone library once `init_with_env` has succeeded, never before.
pub fn platform_ready() {
    READY.store(true, Ordering::Release);
}

/// Whether Android's trust store is asked ([`platform_ready`] was called).
pub fn platform_is_ready() -> bool {
    READY.load(Ordering::Acquire)
}

fn platform() -> Option<&'static rustls_platform_verifier::Verifier> {
    static VERIFIER: OnceLock<Option<rustls_platform_verifier::Verifier>> = OnceLock::new();
    if !READY.load(Ordering::Acquire) {
        return None;
    }
    VERIFIER
        .get_or_init(|| match rustls_platform_verifier::Verifier::new(Arc::new(rustls::crypto::ring::default_provider())) {
            Ok(v) => Some(v),
            Err(e) => {
                log::warn!("Android's trust store can't be asked: {e}");
                None
            }
        })
        .as_ref()
}

/// Android's verdict on a chain Mozilla's roots refused as `refused`: asked only when the issuer
/// was unknown to them, and only once the platform's side is set up.
pub(super) fn second_opinion(
    refused: &Error,
    end_entity: &CertificateDer<'_>,
    intermediates: &[CertificateDer<'_>],
    server_name: &ServerName<'_>,
    ocsp_response: &[u8],
    now: UnixTime,
) -> Option<ServerCertVerified> {
    if !matches!(refused, Error::InvalidCertificate(CertificateError::UnknownIssuer)) {
        return None;
    }
    let v = platform()?;
    match v.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now) {
        Ok(ok) => {
            log::info!("a server's certificate was vouched for by Android's trust store, not Mozilla's roots");
            Some(ok)
        }
        Err(_) => None,
    }
}
