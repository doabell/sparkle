use crate::settings::Settings;
use futures_util::StreamExt;
use object_store::aws::AmazonS3Builder;
use object_store::path::Path;
use object_store::{Attribute, AttributeValue, Attributes, ObjectStore, PutOptions, PutPayload};
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
/// driven by a small runtime owned by this store. The object listing is cached
/// for the lifetime of the worker; each content hash is therefore listed once
/// and uploaded at most once per process.
pub(crate) struct S3ArtworkStore {
    config: S3Config,
    store: Arc<dyn ObjectStore>,
    runtime: Runtime,
    existing_keys: Option<HashSet<String>>,
}

#[derive(Clone, Debug)]
struct S3Config {
    public_url: Url,
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
    /// S3 is deliberately opt-in. A missing endpoint and bucket means the
    /// application is using its legacy Catbox path; a partially configured
    /// store is treated as a configuration error so it cannot silently cause
    /// new uploads to go somewhere unexpected.
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

        Self::new(
            endpoint,
            bucket,
            public_url,
            access_key,
            secret_key,
            session_token,
            region,
            prefix,
        )
        .map(Some)
    }

    fn new(
        endpoint: Url,
        bucket: String,
        public_url: Url,
        access_key: Option<String>,
        secret_key: Option<String>,
        session_token: Option<String>,
        region: String,
        prefix: String,
    ) -> Result<Self, String> {
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
            existing_keys: None,
        })
    }

    /// Returns a public URL for an existing object or uploads the object under
    /// the first stable content hash when the listing has no match.
    pub(crate) fn find_or_upload(
        &mut self,
        jpeg: Vec<u8>,
        content_hashes: &[String],
    ) -> Result<String, String> {
        if self.existing_keys.is_none() {
            self.existing_keys = Some(self.list_existing_keys()?);
        }
        let existing_keys = self
            .existing_keys
            .as_ref()
            .expect("S3 object listing is initialized");
        if let Some(object_key) = existing_object_key(existing_keys, &self.config, content_hashes) {
            return Ok(self.public_url(&object_key));
        }

        let hash = content_hashes
            .iter()
            .find(|hash| is_content_hash(hash))
            .ok_or_else(|| "artwork has no valid content hash".to_string())?;
        let object_key = self.config.object_key(hash);
        self.put_object(&object_key, jpeg)?;
        self.existing_keys
            .as_mut()
            .expect("S3 object listing is initialized")
            .insert(object_key.clone());
        Ok(self.public_url(&object_key))
    }

    fn list_existing_keys(&self) -> Result<HashSet<String>, String> {
        let store = Arc::clone(&self.store);
        let prefix =
            (!self.config.prefix.is_empty()).then(|| Path::from(self.config.prefix.clone()));
        let configured_prefix = self.config.prefix.clone();
        self.runtime.block_on(async move {
            let mut entries = store.list(prefix.as_ref());
            let mut keys = HashSet::new();
            while let Some(result) = entries.next().await {
                let entry = result.map_err(|err| format!("S3 list failed: {err}"))?;
                let key = entry.location.to_string();
                if key.starts_with(&configured_prefix) {
                    keys.insert(key);
                }
            }
            Ok(keys)
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

    fn public_url(&self, object_key: &str) -> String {
        append_path(&self.config.public_url, &encode_path(object_key)).to_string()
    }
}

impl S3Config {
    fn object_key(&self, hash: &str) -> String {
        format!("{}{hash}.jpg", self.prefix)
    }
}

fn existing_object_key(
    existing_keys: &HashSet<String>,
    config: &S3Config,
    content_hashes: &[String],
) -> Option<String> {
    content_hashes
        .iter()
        .filter(|hash| is_content_hash(hash))
        .map(|hash| config.object_key(hash))
        .find(|key| existing_keys.contains(key))
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

    fn config(prefix: &str) -> S3Config {
        S3Config {
            public_url: Url::parse("https://cdn.example.test").unwrap(),
            prefix: normalize_prefix(prefix),
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
    fn existing_lookup_ignores_album_keys_and_reuses_any_content_hash() {
        let config = config("");
        let existing = HashSet::from(["abcdefabcdefabcdefabcdefabcdefab.jpg".to_string()]);
        let hashes = vec![
            "album:42".to_string(),
            "abcdefabcdefabcdefabcdefabcdefab".to_string(),
        ];
        assert_eq!(
            existing_object_key(&existing, &config, &hashes),
            Some("abcdefabcdefabcdefabcdefabcdefab.jpg".to_string())
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
    fn public_path_style_store_can_be_built_without_network_access() {
        let store = S3ArtworkStore::new(
            Url::parse("http://minio.example.test:9000").unwrap(),
            "sparkle".to_string(),
            Url::parse("https://cdn.example.test").unwrap(),
            None,
            None,
            None,
            DEFAULT_REGION.to_string(),
            normalize_prefix("artwork"),
        )
        .unwrap();
        assert_eq!(store.config.prefix, "artwork/");
    }

    #[test]
    fn persisted_settings_build_an_s3_store_without_network_access() {
        let mut settings = Settings::default();
        settings.discord_artwork_s3_endpoint = "http://minio.example.test:9000".to_string();
        settings.discord_artwork_s3_bucket = "sparkle".to_string();
        settings.discord_artwork_s3_access_key = "access".to_string();
        settings.discord_artwork_s3_secret_key = "secret".to_string();
        settings.discord_artwork_s3_prefix = "artwork".to_string();

        let store = S3ArtworkStore::from_settings(&settings)
            .unwrap()
            .expect("settings should enable S3");
        assert_eq!(store.config.prefix, "artwork/");
    }
}
