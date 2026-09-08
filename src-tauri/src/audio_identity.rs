//! A track's identity follows its encoded audio, not its filename or tags.
//! Demuxing skips ID3, Vorbis comments, MP4 metadata and artwork without the
//! cost of decoding every sample. This is cached until the file changes.

use sha2::{Digest, Sha256};
use std::fs::File;
use std::path::Path;
use symphonia::core::errors::Error;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::probe::Hint;

pub fn fingerprint(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|s| s.to_str()) {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe()
        .format(&hint, stream, &Default::default(), &Default::default())
        .map_err(|e| e.to_string())?
        .format;
    let track = format.default_track().ok_or("no audio stream")?;
    let track_id = track.id;
    let mut digest = Sha256::new();
    digest.update(format!("{:?}", track.codec_params.codec).as_bytes());
    let mut packets = 0_u64;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id() == track_id {
            digest.update((packet.data.len() as u64).to_le_bytes());
            digest.update(&packet.data);
            packets += 1;
        }
    }
    if packets == 0 {
        return Err("no audio packets".into());
    }
    Ok(format!("audio-v1:{:x}", digest.finalize()))
}
