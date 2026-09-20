use std::sync::Once;

/// reqwest's provider-free Rustls backend needs this before constructing clients.
/// Share one provider with S3's internal client and the updater's HTTP transport.
pub(crate) fn initialize_tls() {
    static INITIALIZE: Once = Once::new();
    INITIALIZE.call_once(|| {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("install ring TLS crypto provider before constructing HTTP clients");
    });
}

pub(crate) fn builder() -> reqwest::blocking::ClientBuilder {
    initialize_tls();
    reqwest::blocking::Client::builder()
}

#[cfg(test)]
#[path = "tests/http_client.rs"]
mod tests;
