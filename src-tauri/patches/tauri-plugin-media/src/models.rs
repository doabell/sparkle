use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMetadata {
    pub title: String,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub album_artist: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>, // Duration in seconds
    #[serde(default)]
    pub artwork_url: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "base64_serde"
    )]
    pub artwork_data: Option<Vec<u8>>, // Raw image data
}

mod base64_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(data: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match data {
            Some(bytes) => {
                let encoded = BASE64.encode(bytes);
                serializer.serialize_str(&encoded)
            }
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: Option<String> = Option::deserialize(deserializer)?;
        match s {
            Some(encoded) if encoded.is_empty() => Ok(None),
            Some(encoded) => BASE64
                .decode(encoded)
                .map(Some)
                .map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MediaMetadata;

    #[test]
    fn optional_metadata_fields_can_be_omitted_from_ipc_payload() {
        let metadata: MediaMetadata =
            serde_json::from_str(r#"{"title":"Song","duration":123}"#).unwrap();

        assert_eq!(metadata.title, "Song");
        assert!(metadata.artist.is_none());
        assert!(metadata.artwork_data.is_none());
    }

    #[test]
    fn empty_artwork_data_is_treated_as_missing() {
        let metadata: MediaMetadata =
            serde_json::from_str(r#"{"title":"Song","artworkData":""}"#).unwrap();

        assert!(metadata.artwork_data.is_none());
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackInfo {
    pub status: PlaybackStatus,
    pub position: f64, // Current position in seconds
    pub shuffle: bool,
    pub repeat_mode: RepeatMode,
    pub playback_rate: f64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum RepeatMode {
    None,
    Track,
    List,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaControlEvent {
    pub event_type: MediaControlEventType,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaControlEventType {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    FastForward,
    Rewind,
    SeekTo(f64),
    SetPosition(f64),
    SetPlaybackRate(f64),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeMediaSessionRequest {
    pub app_id: String,
    pub app_name: String,
}
