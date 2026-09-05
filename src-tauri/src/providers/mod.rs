pub mod album_art;
pub mod artist;
pub mod lyrics;

use std::io::Read;

/// Artwork comes from third-party hosts, so never let a response allocate an
/// unbounded byte buffer before the image decoder gets a chance to validate it.
pub fn read_image_response(response: reqwest::blocking::Response) -> Result<Vec<u8>, String> {
    const MAX_IMAGE_DOWNLOAD_BYTES: usize = 20 * 1024 * 1024;

    let declared_length = response.content_length();
    if let Some(length) = declared_length {
        if length > MAX_IMAGE_DOWNLOAD_BYTES as u64 {
            return Err("image is too large (max 20 MB)".to_string());
        }
    }

    let initial_capacity = declared_length
        .unwrap_or_default()
        .min(MAX_IMAGE_DOWNLOAD_BYTES as u64) as usize;
    let mut data = Vec::with_capacity(initial_capacity);
    let mut response = response.take(MAX_IMAGE_DOWNLOAD_BYTES as u64 + 1);
    response.read_to_end(&mut data).map_err(|e| e.to_string())?;
    if data.len() > MAX_IMAGE_DOWNLOAD_BYTES {
        return Err("image is too large (max 20 MB)".to_string());
    }
    Ok(data)
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
