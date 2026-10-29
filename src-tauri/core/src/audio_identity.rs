//! A track's identity follows its encoded audio, not its filename or tags.
//! Demuxing skips ID3, Vorbis comments, MP4 metadata and artwork without the
//! cost of decoding every sample. This is cached until the file changes.

use sha2::{Digest, Sha256};
use std::fs::File;
use std::path::Path;
use symphonia::core::codecs::audio::{well_known::*, AudioCodecId, CODEC_ID_NULL_AUDIO};
use symphonia::core::errors::Error;
use symphonia::core::formats::{probe::Hint, TrackType};
use symphonia::core::io::MediaSourceStream;

pub const FINGERPRINT_PREFIX: &str = "audio-v1:";

// audio-v1 stored Symphonia 0.5's Debug representation in the hash. Version 0.6
// renames the wrapper and renumbers lossy codecs, so freeze the original tags
// for codecs recognized by our FLAC, MPEG, MP4, and Ogg readers.
fn legacy_codec_tag(codec: AudioCodecId) -> Option<&'static str> {
    Some(match codec {
        CODEC_ID_NULL_AUDIO => "CodecType(0)",
        CODEC_ID_PCM_S32LE => "CodecType(256)",
        CODEC_ID_PCM_S32BE => "CodecType(258)",
        CODEC_ID_PCM_S24LE => "CodecType(260)",
        CODEC_ID_PCM_S24BE => "CodecType(262)",
        CODEC_ID_PCM_S16LE => "CodecType(264)",
        CODEC_ID_PCM_S16BE => "CodecType(266)",
        CODEC_ID_PCM_S8 => "CodecType(268)",
        CODEC_ID_PCM_U32LE => "CodecType(270)",
        CODEC_ID_PCM_U32BE => "CodecType(272)",
        CODEC_ID_PCM_U24LE => "CodecType(274)",
        CODEC_ID_PCM_U24BE => "CodecType(276)",
        CODEC_ID_PCM_U16LE => "CodecType(278)",
        CODEC_ID_PCM_U16BE => "CodecType(280)",
        CODEC_ID_PCM_U8 => "CodecType(282)",
        CODEC_ID_PCM_F32LE => "CodecType(284)",
        CODEC_ID_PCM_F32BE => "CodecType(286)",
        CODEC_ID_PCM_F64LE => "CodecType(288)",
        CODEC_ID_PCM_F64BE => "CodecType(290)",
        CODEC_ID_VORBIS => "CodecType(4096)",
        CODEC_ID_MP1 => "CodecType(4097)",
        CODEC_ID_MP2 => "CodecType(4098)",
        CODEC_ID_MP3 => "CodecType(4099)",
        CODEC_ID_AAC => "CodecType(4100)",
        CODEC_ID_OPUS => "CodecType(4101)",
        CODEC_ID_FLAC => "CodecType(8192)",
        CODEC_ID_ALAC => "CodecType(8195)",
        _ => return None,
    })
}

pub fn fingerprint(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|s| s.to_str()) {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, stream, Default::default(), Default::default())
        .map_err(|e| e.to_string())?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or("no audio stream")?;
    let track_id = track.id;
    let codec = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or("no audio codec parameters")?
        .codec;
    let mut digest = Sha256::new();
    digest.update(legacy_codec_tag(codec).ok_or("unsupported audio fingerprint codec")?);
    let mut packets = 0_u64;
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id == track_id {
            digest.update((packet.data.len() as u64).to_le_bytes());
            digest.update(&packet.data);
            packets += 1;
        }
    }
    if packets == 0 {
        return Err("no audio packets".into());
    }
    let hex: String = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(format!("{FINGERPRINT_PREFIX}{hex}"))
}

#[cfg(test)]
#[path = "tests/audio_identity.rs"]
mod tests;
