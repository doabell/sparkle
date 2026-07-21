//! Windows-only GDI+ artwork normalization for Sparkle's Catbox cache.
//!
//! The cache key is the MD5 of the resulting JPEG's base64 text, so a visually
//! identical JPEG from a different encoder would still miss an existing entry.

use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use windows_sys::core::GUID;
use windows_sys::Win32::Graphics::GdiPlus::{
    CompositingModeSourceCopy, CompositingQualityHighQuality, EncoderParameter,
    EncoderParameterValueTypeLong, EncoderParameters, EncoderQuality, GdipBitmapSetResolution,
    GdipCreateBitmapFromScan0, GdipCreateImageAttributes, GdipDeleteGraphics, GdipDisposeImage,
    GdipDisposeImageAttributes, GdipDrawImageRectRectI, GdipGetImageGraphicsContext,
    GdipGetImageHeight, GdipGetImageHorizontalResolution, GdipGetImageVerticalResolution,
    GdipGetImageWidth, GdipLoadImageFromFileICM, GdipSaveImageToFile, GdipSetCompositingMode,
    GdipSetCompositingQuality, GdipSetImageAttributesWrapMode, GdipSetInterpolationMode,
    GdipSetPixelOffsetMode, GdipSetSmoothingMode, GdiplusStartup, GdiplusStartupInput, GpBitmap,
    GpGraphics, GpImage, GpImageAttributes, InterpolationModeHighQualityBicubic, Ok as GdiOk,
    PixelOffsetModeHighQuality, SmoothingModeHighQuality, Status, UnitPixel, WrapModeTileFlipXY,
};

const FORMAT_32BPP_ARGB: i32 = 0x26200a;
const JPEG_ENCODER: GUID = GUID::from_u128(0x557cf401_1a04_11d3_9a73_0000f81ef32e);

static GDIPLUS_INITIALIZATION: OnceLock<Result<usize, String>> = OnceLock::new();

struct ImageHandle(*mut GpImage);

impl Drop for ImageHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                GdipDisposeImage(self.0);
            }
        }
    }
}

struct GraphicsHandle(*mut GpGraphics);

impl Drop for GraphicsHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                GdipDeleteGraphics(self.0);
            }
        }
    }
}

struct ImageAttributesHandle(*mut GpImageAttributes);

impl Drop for ImageAttributesHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                GdipDisposeImageAttributes(self.0);
            }
        }
    }
}

/// Produces Sparkle's stable GDI+ resize/JPEG pipeline for cache lookups.
pub fn resize_to_cache_jpeg(
    original: &[u8],
    work_dir: &Path,
    cache_key: &str,
    max_dimension: u32,
    jpeg_quality: u8,
) -> Result<Vec<u8>, String> {
    ensure_gdiplus_started()?;
    fs::create_dir_all(work_dir).map_err(|err| err.to_string())?;

    let temporary_stem = format!(".sparkle-artwork-{}-{}", std::process::id(), cache_key);
    let input_path = work_dir.join(format!("{temporary_stem}.source"));
    let output_path = work_dir.join(format!("{temporary_stem}.jpg"));
    let _ = fs::remove_file(&output_path);
    fs::write(&input_path, original).map_err(|err| err.to_string())?;

    let result = (|| {
        resize_file(&input_path, &output_path, max_dimension, jpeg_quality)?;
        fs::read(&output_path).map_err(|err| err.to_string())
    })();

    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_path);
    result
}

fn ensure_gdiplus_started() -> Result<(), String> {
    let initialization = GDIPLUS_INITIALIZATION.get_or_init(|| {
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: 0,
            SuppressExternalCodecs: 0,
        };
        let mut token = 0;
        let status = unsafe { GdiplusStartup(&mut token, &input, null_mut()) };
        if status == GdiOk {
            Ok(token)
        } else {
            Err(format!("GDI+ startup failed with status {status}"))
        }
    });
    initialization.as_ref().map(|_| ()).map_err(Clone::clone)
}

fn resize_file(
    input_path: &Path,
    output_path: &Path,
    max_dimension: u32,
    jpeg_quality: u8,
) -> Result<(), String> {
    let input_path = wide_path(input_path);
    let output_path = wide_path(output_path);
    let mut source = null_mut();
    check_status(
        unsafe { GdipLoadImageFromFileICM(input_path.as_ptr(), &mut source) },
        "load source image",
    )?;
    if source.is_null() {
        return Err("GDI+ returned no source image".to_string());
    }
    let source = ImageHandle(source);

    let source_width = image_dimension(source.0, GdipGetImageWidth, "read source width")?;
    let source_height = image_dimension(source.0, GdipGetImageHeight, "read source height")?;
    let horizontal_resolution = image_resolution(
        source.0,
        GdipGetImageHorizontalResolution,
        "read source horizontal resolution",
    )?;
    let vertical_resolution = image_resolution(
        source.0,
        GdipGetImageVerticalResolution,
        "read source vertical resolution",
    )?;

    let ratio = (max_dimension as f64 / source_width as f64)
        .min(max_dimension as f64 / source_height as f64);
    let target_width = dotnet_round_to_i32(source_width as f64 * ratio).max(1);
    let target_height = dotnet_round_to_i32(source_height as f64 * ratio).max(1);

    let mut bitmap: *mut GpBitmap = null_mut();
    check_status(
        unsafe {
            GdipCreateBitmapFromScan0(
                target_width,
                target_height,
                0,
                FORMAT_32BPP_ARGB,
                null(),
                &mut bitmap,
            )
        },
        "create destination bitmap",
    )?;
    if bitmap.is_null() {
        return Err("GDI+ returned no destination bitmap".to_string());
    }
    let destination = ImageHandle(bitmap.cast());
    check_status(
        unsafe { GdipBitmapSetResolution(bitmap, horizontal_resolution, vertical_resolution) },
        "set destination resolution",
    )?;

    let mut graphics = null_mut();
    check_status(
        unsafe { GdipGetImageGraphicsContext(destination.0, &mut graphics) },
        "create drawing context",
    )?;
    if graphics.is_null() {
        return Err("GDI+ returned no drawing context".to_string());
    }
    let graphics = GraphicsHandle(graphics);
    check_status(
        unsafe { GdipSetCompositingMode(graphics.0, CompositingModeSourceCopy) },
        "set compositing mode",
    )?;
    check_status(
        unsafe { GdipSetCompositingQuality(graphics.0, CompositingQualityHighQuality) },
        "set compositing quality",
    )?;
    check_status(
        unsafe { GdipSetInterpolationMode(graphics.0, InterpolationModeHighQualityBicubic) },
        "set interpolation mode",
    )?;
    check_status(
        unsafe { GdipSetSmoothingMode(graphics.0, SmoothingModeHighQuality) },
        "set smoothing mode",
    )?;
    check_status(
        unsafe { GdipSetPixelOffsetMode(graphics.0, PixelOffsetModeHighQuality) },
        "set pixel offset mode",
    )?;

    let mut attributes = null_mut();
    check_status(
        unsafe { GdipCreateImageAttributes(&mut attributes) },
        "create image attributes",
    )?;
    if attributes.is_null() {
        return Err("GDI+ returned no image attributes".to_string());
    }
    let attributes = ImageAttributesHandle(attributes);
    check_status(
        unsafe { GdipSetImageAttributesWrapMode(attributes.0, WrapModeTileFlipXY, 0, 0) },
        "set image wrap mode",
    )?;
    check_status(
        unsafe {
            GdipDrawImageRectRectI(
                graphics.0,
                source.0,
                0,
                0,
                target_width,
                target_height,
                0,
                0,
                source_width as i32,
                source_height as i32,
                UnitPixel,
                attributes.0,
                0,
                null_mut(),
            )
        },
        "resize artwork",
    )?;

    let mut quality = jpeg_quality as u32;
    let encoder_parameters = EncoderParameters {
        Count: 1,
        Parameter: [EncoderParameter {
            Guid: EncoderQuality,
            NumberOfValues: 1,
            Type: EncoderParameterValueTypeLong as u32,
            Value: (&mut quality as *mut u32).cast(),
        }],
    };
    check_status(
        unsafe {
            GdipSaveImageToFile(
                destination.0,
                output_path.as_ptr(),
                &JPEG_ENCODER,
                &encoder_parameters,
            )
        },
        "encode JPEG",
    )
}

fn image_dimension(
    image: *mut GpImage,
    get_dimension: unsafe extern "system" fn(*mut GpImage, *mut u32) -> Status,
    operation: &str,
) -> Result<u32, String> {
    let mut dimension = 0;
    check_status(unsafe { get_dimension(image, &mut dimension) }, operation)?;
    if dimension == 0 {
        return Err(format!("GDI+ {operation} returned zero"));
    }
    Ok(dimension)
}

fn image_resolution(
    image: *mut GpImage,
    get_resolution: unsafe extern "system" fn(*mut GpImage, *mut f32) -> Status,
    operation: &str,
) -> Result<f32, String> {
    let mut resolution = 0.0;
    check_status(unsafe { get_resolution(image, &mut resolution) }, operation)?;
    if resolution <= 0.0 {
        return Err(format!("GDI+ {operation} returned an invalid value"));
    }
    Ok(resolution)
}

fn dotnet_round_to_i32(value: f64) -> i32 {
    value.round_ties_even() as i32
}

fn check_status(status: Status, operation: &str) -> Result<(), String> {
    if status == GdiOk {
        Ok(())
    } else {
        Err(format!("GDI+ could not {operation} (status {status})"))
    }
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
