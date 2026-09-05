use std::path::{Path, PathBuf};

/// Owns only a uniquely created test directory; cleanup also runs on assertion failure.
pub(crate) struct TestDir(PathBuf);

impl TestDir {
    pub(crate) fn new() -> Self {
        let path = std::env::temp_dir().join(crate::analytics::new_trace_id("sparkle-test"));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    pub(crate) fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    pub(crate) fn audio(&self, name: &str) -> PathBuf {
        let path = self.join(name);
        std::fs::write(&path, include_bytes!("fixtures/tone.flac")).unwrap();
        path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A bounded, one-response HTTP peer. It never opens a public interface and
/// joins on drop; a missing request fails within five seconds, not indefinitely.
pub(crate) struct HttpFixture {
    pub(crate) url: String,
    worker: Option<std::thread::JoinHandle<String>>,
}

impl HttpFixture {
    pub(crate) fn new(response: Vec<u8>) -> Self {
        use std::io::{Read, Write};
        use std::time::{Duration, Instant};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("fixture request did not arrive: {e}"),
                }
            };
            // Windows may inherit the listener's nonblocking mode.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                assert!(request.len() < 16_384, "oversized fixture request");
            }
            // A bounded client may reject a body before consuming all bytes.
            let _ = stream.write_all(&response);
            String::from_utf8(request).unwrap()
        });
        Self {
            url,
            worker: Some(worker),
        }
    }

    pub(crate) fn response(status: u16, content_type: &str, body: &[u8]) -> Self {
        let mut response = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).into_bytes();
        response.extend_from_slice(body);
        Self::new(response)
    }

    pub(crate) fn json(status: u16, body: &str) -> Self {
        Self::response(status, "application/json", body.as_bytes())
    }

    pub(crate) fn request(mut self) -> String {
        self.worker.take().unwrap().join().unwrap()
    }
}

impl Drop for HttpFixture {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            // Do not hide a broken fixture behind an expected provider error,
            // but avoid a second panic while an assertion is already unwinding.
            if !std::thread::panicking() {
                result.expect("HTTP fixture failed");
            }
        }
    }
}

pub(crate) fn http_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap()
}
