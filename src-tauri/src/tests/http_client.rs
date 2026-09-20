use super::*;
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

// This self-signed certificate and public test key are trusted only by these tests.
const CERTIFICATE: &[u8] = include_bytes!("fixtures/localhost-cert.pem");
const PRIVATE_KEY: &[u8] = include_bytes!("fixtures/localhost-key.pem");

struct TlsFixture {
    url: String,
    worker: Option<std::thread::JoinHandle<bool>>,
}

impl TlsFixture {
    fn new(version: &'static rustls::SupportedProtocolVersion) -> Self {
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[version])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from_pem_slice(CERTIFICATE).unwrap()],
            PrivateKeyDer::from_pem_slice(PRIVATE_KEY).unwrap(),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "https://localhost:{}",
            listener.local_addr().unwrap().port()
        );
        listener.set_nonblocking(true).unwrap();
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("TLS fixture did not receive a connection: {error}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let connection = rustls::ServerConnection::new(Arc::new(config)).unwrap();
            let mut stream = rustls::StreamOwned::new(connection, socket);
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                // A client rejecting the certificate sends no HTTP request.
                if stream.read_exact(&mut byte).is_err() {
                    return false;
                }
                request.push(byte[0]);
                assert!(request.len() < 16_384, "oversized TLS fixture request");
            }
            assert!(request.starts_with(b"GET / HTTP/1.1\r\n"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
            stream.flush().unwrap();
            true
        });
        Self {
            url,
            worker: Some(worker),
        }
    }

    fn received_request(mut self) -> bool {
        self.worker.take().unwrap().join().unwrap()
    }
}

impl Drop for TlsFixture {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !std::thread::panicking() {
                result.expect("TLS fixture failed");
            }
        }
    }
}

#[test]
fn trusted_https_works_with_tls12_and_tls13() {
    let client = builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .tls_certs_only([reqwest::Certificate::from_pem(CERTIFICATE).unwrap()])
        .build()
        .unwrap();
    for version in [&rustls::version::TLS12, &rustls::version::TLS13] {
        let peer = TlsFixture::new(version);
        let response = client.get(&peer.url).send().unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.text().unwrap(), "ok");
        assert!(peer.received_request());
    }
}

#[test]
fn untrusted_https_certificate_is_rejected() {
    let client = builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let peer = TlsFixture::new(&rustls::version::TLS13);
    assert!(client.get(&peer.url).send().unwrap_err().is_connect());
    assert!(!peer.received_request());
}
