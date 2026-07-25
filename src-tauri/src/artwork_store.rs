use crate::settings::Settings;
use object_store::aws::AmazonS3Builder;
use object_store::path::Path;
use object_store::{
    Attribute, AttributeValue, Attributes, Error as ObjectStoreError, ObjectStore, ObjectStoreExt,
    PutOptions, PutPayload,
};
use reqwest::Url;
use std::collections::HashSet;
use std::env;
use std::sync::Arc;
use tokio::runtime::Runtime;

const ENDPOINT_ENV: &str = "SPARKLE_ARTWORK_S3_ENDPOINT";
const BUCKET_ENV: &str = "SPARKLE_ARTWORK_S3_BUCKET";
const PUBLIC_URL_ENV: &str = "SPARKLE_ARTWORK_S3_PUBLIC_URL";
const ACCESS_KEY_ENV: &str = "SPARKLE_ARTWORK_S3_ACCESS_KEY";
const SECRET_KEY_ENV: &str = "SPARKLE_ARTWORK_S3_SECRET_KEY";
const SESSION_TOKEN_ENV: &str = "SPARKLE_ARTWORK_S3_SESSION_TOKEN";
const REGION_ENV: &str = "SPARKLE_ARTWORK_S3_REGION";
const PREFIX_ENV: &str = "SPARKLE_ARTWORK_S3_PREFIX";
const DEFAULT_REGION: &str = "us-east-1";
const DEFAULT_PREFIX: &str = "sparkle/";

/// S3-compatible artwork storage for the Discord presence worker.
///
/// The worker is a synchronous thread, so the async object_store client is
/// driven by a small runtime owned by this store. Only deterministic artwork
/// keys are probed; the configured prefix is never enumerated.
pub(crate) struct S3ArtworkStore {
    config: S3Config,
    store: Arc<dyn ObjectStore>,
    runtime: Runtime,
    known_keys: HashSet<String>,
}

#[derive(Clone, Debug)]
struct S3Config {
    public_url: Url,
    prefix: String,
}

struct S3BuildConfig {
    endpoint: Url,
    bucket: String,
    public_url: Url,
    access_key: Option<String>,
    secret_key: Option<String>,
    session_token: Option<String>,
    region: String,
    prefix: String,
}

#[derive(Default)]
struct S3Values {
    endpoint: Option<String>,
    bucket: Option<String>,
    public_url: Option<String>,
    access_key: Option<String>,
    secret_key: Option<String>,
    session_token: Option<String>,
    region: Option<String>,
    prefix: Option<String>,
}

impl S3Values {
    fn from_settings(settings: &Settings) -> Self {
        Self {
            endpoint: non_empty_value(&settings.discord_artwork_s3_endpoint),
            bucket: non_empty_value(&settings.discord_artwork_s3_bucket),
            public_url: non_empty_value(&settings.discord_artwork_s3_public_url),
            access_key: non_empty_value(&settings.discord_artwork_s3_access_key),
            secret_key: non_empty_value(&settings.discord_artwork_s3_secret_key),
            session_token: non_empty_value(&settings.discord_artwork_s3_session_token),
            region: non_empty_value(&settings.discord_artwork_s3_region),
            prefix: non_empty_value(&settings.discord_artwork_s3_prefix),
        }
    }

    fn from_environment() -> Self {
        Self {
            endpoint: non_empty_env(ENDPOINT_ENV),
            bucket: non_empty_env(BUCKET_ENV),
            public_url: non_empty_env(PUBLIC_URL_ENV),
            access_key: non_empty_env(ACCESS_KEY_ENV),
            secret_key: non_empty_env(SECRET_KEY_ENV),
            session_token: non_empty_env(SESSION_TOKEN_ENV),
            region: non_empty_env(REGION_ENV),
            prefix: non_empty_env(PREFIX_ENV),
        }
    }

    fn is_configured(&self) -> bool {
        [
            &self.endpoint,
            &self.bucket,
            &self.public_url,
            &self.access_key,
            &self.secret_key,
            &self.session_token,
            &self.region,
            &self.prefix,
        ]
        .iter()
        .any(|value| value.is_some())
    }
}

impl S3ArtworkStore {
    /// Builds a store from persisted settings, retaining environment variables
    /// as a deployment-friendly fallback when the UI settings are empty.
    ///
    /// S3 is deliberately opt-in. A missing endpoint and bucket means S3 is
    /// not configured; the caller decides whether that is acceptable for the
    /// selected artwork mode. A partially configured store is treated as a
    /// configuration error so it cannot silently cause new uploads to go
    /// somewhere unexpected.
    pub(crate) fn from_settings(settings: &Settings) -> Result<Option<Self>, String> {
        let values = S3Values::from_settings(settings);
        if values.is_configured() {
            Self::from_values(values)
        } else {
            Self::from_values(S3Values::from_environment())
        }
    }

    fn from_values(values: S3Values) -> Result<Option<Self>, String> {
        let endpoint = values.endpoint;
        let bucket = values.bucket;
        if endpoint.is_none() && bucket.is_none() {
            return Ok(None);
        }
        let endpoint = endpoint.ok_or_else(|| "S3 artwork endpoint is not set".to_string())?;
        let bucket = bucket.ok_or_else(|| "S3 artwork bucket is not set".to_string())?;
        let public_url = values.public_url;
        let access_key = values.access_key;
        let secret_key = values.secret_key;
        let session_token = values.session_token;
        if access_key.is_some() != secret_key.is_some() {
            return Err(
                "S3 artwork access and secret keys must be configured together".to_string(),
            );
        }
        if session_token.is_some() && access_key.is_none() {
            return Err("S3 artwork session token requires access and secret keys".to_string());
        }

        let endpoint = parse_base_url(&endpoint, "S3 artwork endpoint")?;
        let public_url = match public_url {
            Some(value) => parse_base_url(&value, "S3 artwork public URL")?,
            None => append_path(&endpoint, &encode_path(&bucket)),
        };
        let prefix = normalize_prefix(&values.prefix.unwrap_or_else(|| DEFAULT_PREFIX.to_string()));
        let region = values.region.unwrap_or_else(|| DEFAULT_REGION.to_string());

        Self::new(S3BuildConfig {
            endpoint,
            bucket,
            public_url,
            access_key,
            secret_key,
            session_token,
            region,
            prefix,
        })
        .map(Some)
    }

    fn new(config: S3BuildConfig) -> Result<Self, String> {
        let S3BuildConfig {
            endpoint,
            bucket,
            public_url,
            access_key,
            secret_key,
            session_token,
            region,
            prefix,
        } = config;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("failed to create S3 runtime: {err}"))?;

        let mut builder = AmazonS3Builder::new()
            .with_bucket_name(bucket)
            .with_endpoint(endpoint.to_string())
            .with_region(region)
            // S3-compatible services commonly expose path-style endpoints.
            .with_virtual_hosted_style_request(false);
        if endpoint.scheme() == "http" {
            builder = builder.with_allow_http(true);
        }
        match (access_key, secret_key) {
            (Some(access_key), Some(secret_key)) => {
                builder = builder
                    .with_access_key_id(access_key)
                    .with_secret_access_key(secret_key);
                if let Some(session_token) = session_token {
                    builder = builder.with_token(session_token);
                }
            }
            (None, None) => {
                // Do not let the client discover unrelated AWS credentials
                // from the host when the configured bucket is intentionally public.
                builder = builder.with_skip_signature(true);
            }
            _ => unreachable!("S3 credentials are validated together"),
        }

        let store = builder
            .build()
            .map_err(|err| format!("failed to create S3 store: {err}"))?;
        Ok(Self {
            config: S3Config { public_url, prefix },
            store: Arc::new(store),
            runtime,
            known_keys: HashSet::new(),
        })
    }

    /// Returns a public URL for an existing object or uploads the object under
    /// the first stable content hash when bounded HEAD probes find no match.
    pub(crate) fn find_or_upload(
        &mut self,
        jpeg: Vec<u8>,
        content_hashes: &[String],
    ) -> Result<String, String> {
        let object_keys = candidate_object_keys(&self.config, content_hashes);
        if object_keys.is_empty() {
            return Err("artwork has no valid content hash".to_string());
        }

        for object_key in &object_keys {
            if self.known_keys.contains(object_key) || self.object_exists(object_key)? {
                self.known_keys.insert(object_key.clone());
                return Ok(self.public_url(object_key));
            }
        }

        let object_key = &object_keys[0];
        self.put_object(object_key, jpeg)?;
        self.known_keys.insert(object_key.clone());
        Ok(self.public_url(object_key))
    }

    /// Uploads and then HEADs a deterministic test object. This verifies both
    /// write and read access without requiring permission to list the bucket.
    /// The probe is deleted before this returns, including when verification
    /// fails after the upload.
    pub(crate) fn test_access_and_upload(&mut self, jpeg: Vec<u8>) -> Result<String, String> {
        let object_key = self.config.object_key("sparkle-test");
        let test_result = self.put_object(&object_key, jpeg).and_then(|()| {
            if self.object_exists(&object_key)? {
                Ok(self.public_url(&object_key))
            } else {
                Err("S3 test object was not readable after upload".to_string())
            }
        });
        let cleanup_result = self.delete_object(&object_key);
        self.known_keys.remove(&object_key);

        finish_test_with_cleanup(test_result, cleanup_result, &object_key)
    }

    pub(crate) fn owns_public_url(&self, url: &str) -> bool {
        let base = self.config.public_url.as_str().trim_end_matches('/');
        url.starts_with(&format!("{base}/"))
    }

    fn object_exists(&self, object_key: &str) -> Result<bool, String> {
        let store = Arc::clone(&self.store);
        let location = Path::from(object_key.to_string());
        self.runtime.block_on(async move {
            match store.head(&location).await {
                Ok(_) => Ok(true),
                Err(ObjectStoreError::NotFound { .. }) => Ok(false),
                Err(err) => Err(format!("S3 HEAD failed for {object_key}: {err}")),
            }
        })
    }

    fn put_object(&self, object_key: &str, jpeg: Vec<u8>) -> Result<(), String> {
        let store = Arc::clone(&self.store);
        let location = Path::from(object_key.to_string());
        let mut attributes = Attributes::new();
        attributes.insert(Attribute::ContentType, AttributeValue::from("image/jpeg"));
        let options = PutOptions {
            attributes,
            ..Default::default()
        };
        self.runtime.block_on(async move {
            store
                .put_opts(&location, PutPayload::from(jpeg), options)
                .await
                .map(|_| ())
                .map_err(|err| format!("S3 upload failed: {err}"))
        })
    }

    fn delete_object(&self, object_key: &str) -> Result<(), String> {
        let store = Arc::clone(&self.store);
        let location = Path::from(object_key.to_string());
        self.runtime.block_on(async move {
            match store.delete(&location).await {
                Ok(()) | Err(ObjectStoreError::NotFound { .. }) => Ok(()),
                Err(err) => Err(format!("S3 DELETE failed: {err}")),
            }
        })
    }

    fn public_url(&self, object_key: &str) -> String {
        append_path(&self.config.public_url, &encode_path(object_key)).to_string()
    }
}

fn finish_test_with_cleanup<T>(
    test_result: Result<T, String>,
    cleanup_result: Result<(), String>,
    object_key: &str,
) -> Result<T, String> {
    match (test_result, cleanup_result) {
        (result, Ok(())) => result,
        (Ok(_), Err(cleanup_error)) => Err(format!(
            "S3 access test succeeded, but cleanup failed for test object \
             '{object_key}': {cleanup_error}. Delete it manually from the configured bucket."
        )),
        (Err(test_error), Err(cleanup_error)) => Err(format!(
            "{test_error}; cleanup also failed for test object '{object_key}': \
             {cleanup_error}. Delete it manually from the configured bucket."
        )),
    }
}

impl S3Config {
    fn object_key(&self, hash: &str) -> String {
        format!("{}{hash}.jpg", self.prefix)
    }
}

fn candidate_object_keys(config: &S3Config, content_hashes: &[String]) -> Vec<String> {
    let mut keys = Vec::with_capacity(content_hashes.len());
    for hash in content_hashes.iter().filter(|hash| is_content_hash(hash)) {
        let key = config.object_key(hash);
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

fn is_content_hash(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn non_empty_value(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn non_empty_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_base_url(value: &str, name: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|err| format!("{name} is invalid: {err}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("{name} must use http or https"));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(format!("{name} must not contain a query or fragment"));
    }
    Ok(url)
}

fn normalize_prefix(prefix: &str) -> String {
    let prefix = prefix
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}/")
    }
}

fn append_path(base: &Url, encoded_path: &str) -> Url {
    let mut url = base.clone();
    let base_path = url.path().trim_end_matches('/');
    let path = if base_path.is_empty() {
        format!("/{encoded_path}")
    } else {
        format!("{base_path}/{encoded_path}")
    };
    url.set_path(&path);
    url
}

fn encode_path(value: &str) -> String {
    value
        .split('/')
        .map(aws_encode)
        .collect::<Vec<_>>()
        .join("/")
}

fn aws_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(hex_digit(byte >> 4));
            encoded.push(hex_digit(byte & 0x0f));
        }
    }
    encoded
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'A' + value - 10) as char,
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use object_store::memory::InMemory;

    fn config(prefix: &str) -> S3Config {
        S3Config {
            public_url: Url::parse("https://cdn.example.test").unwrap(),
            prefix: normalize_prefix(prefix),
        }
    }

    fn memory_store(prefix: &str) -> S3ArtworkStore {
        S3ArtworkStore {
            config: config(prefix),
            store: Arc::new(InMemory::new()),
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
            known_keys: HashSet::new(),
        }
    }

    #[test]
    fn content_hashes_map_to_deterministic_jpeg_keys() {
        let config = config("artwork");
        assert_eq!(
            config.object_key("0123456789abcdef0123456789abcdef"),
            "artwork/0123456789abcdef0123456789abcdef.jpg"
        );
    }

    #[test]
    fn candidate_lookup_ignores_album_keys_and_deduplicates_content_hashes() {
        let config = config("");
        let hashes = vec![
            "album:42".to_string(),
            "abcdefabcdefabcdefabcdefabcdefab".to_string(),
            "abcdefabcdefabcdefabcdefabcdefab".to_string(),
        ];
        assert_eq!(
            candidate_object_keys(&config, &hashes),
            vec!["abcdefabcdefabcdefabcdefabcdefab.jpg".to_string()]
        );
    }

    #[test]
    fn prefix_is_normalized_for_stable_object_keys() {
        assert_eq!(normalize_prefix("/artwork//"), "artwork/");
        assert_eq!(normalize_prefix("///"), "");
    }

    #[test]
    fn public_urls_include_the_object_prefix_and_jpeg_name() {
        let config = config("artwork");
        let object_key = config.object_key("0123456789abcdef0123456789abcdef");
        assert_eq!(
            append_path(&config.public_url, &encode_path(&object_key)).as_str(),
            "https://cdn.example.test/artwork/0123456789abcdef0123456789abcdef.jpg"
        );
    }

    #[test]
    fn invalid_cache_keys_are_not_treated_as_content_hashes() {
        assert!(!is_content_hash("album:42"));
        assert!(!is_content_hash("short"));
        assert!(is_content_hash("abcdefabcdefabcdefabcdefabcdefab"));
    }

    #[test]
    fn head_probe_reuses_an_existing_content_object_without_overwriting_it() {
        let hash = "abcdefabcdefabcdefabcdefabcdefab".to_string();
        let mut store = memory_store("artwork");
        let object_key = store.config.object_key(&hash);
        store
            .put_object(&object_key, b"existing-object".to_vec())
            .unwrap();

        let url = store
            .find_or_upload(b"replacement".to_vec(), &[hash])
            .unwrap();

        assert_eq!(
            url,
            "https://cdn.example.test/artwork/abcdefabcdefabcdefabcdefabcdefab.jpg"
        );
        let backend = Arc::clone(&store.store);
        let location = Path::from(object_key);
        let stored = store
            .runtime
            .block_on(async move { backend.get(&location).await.unwrap().bytes().await.unwrap() });
        assert_eq!(stored.as_ref(), b"existing-object");
    }

    #[test]
    fn settings_test_upload_is_verified_and_deleted() {
        let mut store = memory_store("artwork");
        let url = store
            .test_access_and_upload(b"test-object".to_vec())
            .unwrap();

        assert_eq!(url, "https://cdn.example.test/artwork/sparkle-test.jpg");
        assert!(!store.known_keys.contains("artwork/sparkle-test.jpg"));
        assert!(!store
            .object_exists("artwork/sparkle-test.jpg")
            .expect("check that the S3 test object was deleted"));
    }

    #[test]
    fn cleanup_failure_is_actionable_after_a_successful_probe() {
        let error = finish_test_with_cleanup(
            Ok("https://cdn.example.test/artwork/sparkle-test.jpg"),
            Err("S3 DELETE failed: access denied".to_string()),
            "artwork/sparkle-test.jpg",
        )
        .unwrap_err();

        assert!(error.starts_with("S3 access test succeeded, but cleanup failed"));
        assert!(error.contains("artwork/sparkle-test.jpg"));
        assert!(error.contains("S3 DELETE failed: access denied"));
        assert!(error.contains("Delete it manually from the configured bucket"));
    }

    #[test]
    fn cleanup_failure_does_not_mask_the_probe_failure() {
        let error = finish_test_with_cleanup::<String>(
            Err("S3 HEAD failed: timed out".to_string()),
            Err("S3 DELETE failed: access denied".to_string()),
            "artwork/sparkle-test.jpg",
        )
        .unwrap_err();

        assert!(error.starts_with("S3 HEAD failed: timed out"));
        assert!(error.contains("cleanup also failed"));
        assert!(error.contains("artwork/sparkle-test.jpg"));
        assert!(error.contains("S3 DELETE failed: access denied"));
    }

    #[test]
    fn successful_cleanup_preserves_the_probe_failure() {
        let error = finish_test_with_cleanup::<String>(
            Err("S3 test object was not readable after upload".to_string()),
            Ok(()),
            "artwork/sparkle-test.jpg",
        )
        .unwrap_err();

        assert_eq!(
            error,
            "S3 test object was not readable after upload".to_string()
        );
    }

    #[test]
    fn public_path_style_store_can_be_built_without_network_access() {
        let store = S3ArtworkStore::new(S3BuildConfig {
            endpoint: Url::parse("http://minio.example.test:9000").unwrap(),
            bucket: "sparkle".to_string(),
            public_url: Url::parse("https://cdn.example.test").unwrap(),
            access_key: None,
            secret_key: None,
            session_token: None,
            region: DEFAULT_REGION.to_string(),
            prefix: normalize_prefix("artwork"),
        })
        .unwrap();
        assert_eq!(store.config.prefix, "artwork/");
    }

    #[test]
    fn persisted_settings_build_an_s3_store_without_network_access() {
        let settings = Settings {
            discord_artwork_s3_endpoint: "http://minio.example.test:9000".to_string(),
            discord_artwork_s3_bucket: "sparkle".to_string(),
            discord_artwork_s3_access_key: "access".to_string(),
            discord_artwork_s3_secret_key: "secret".to_string(),
            discord_artwork_s3_prefix: "artwork".to_string(),
            ..Default::default()
        };

        let store = S3ArtworkStore::from_settings(&settings)
            .unwrap()
            .expect("settings should enable S3");
        assert_eq!(store.config.prefix, "artwork/");
    }

    #[test]
    fn public_url_ownership_is_scoped_to_the_configured_base() {
        let store = S3ArtworkStore::new(S3BuildConfig {
            endpoint: Url::parse("http://minio.example.test:9000").unwrap(),
            bucket: "sparkle".to_string(),
            public_url: Url::parse("https://cdn.example.test/artwork").unwrap(),
            access_key: None,
            secret_key: None,
            session_token: None,
            region: DEFAULT_REGION.to_string(),
            prefix: normalize_prefix("images"),
        })
        .unwrap();
        assert!(store.owns_public_url("https://cdn.example.test/artwork/images/a.jpg"));
        assert!(!store.owns_public_url("https://cdn.example.test/other/images/a.jpg"));
    }
}
