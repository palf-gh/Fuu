//! Images: decoded with hard limits, scaled down when large, and re-encoded
//! (PNG with transparency, JPEG otherwise) so the renderer only ever decodes
//! output we produced.

use std::io::Cursor;

use image::{DynamicImage, ImageFormat, ImageReader, Limits};

use crate::{Output, PreviewError};

/// Largest image decoded (pixels). Larger headers are refused before decoding.
const MAX_PIXELS: u64 = 50_000_000;
/// Longest side after scaling.
const MAX_SIDE: u32 = 2400;

pub(crate) fn decode(data: &[u8]) -> Result<DynamicImage, PreviewError> {
    let mut reader = ImageReader::new(Cursor::new(data)).with_guessed_format().map_err(|e| PreviewError::Unreadable(e.to_string()))?;
    if reader.format().is_none() {
        return Err(PreviewError::Unreadable("画像の形式を判別できません".into()));
    }
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_PIXELS * 4 + (16 << 20));
    reader.limits(limits);
    let (w, h) = reader.into_dimensions().map_err(|e| PreviewError::Unreadable(e.to_string()))?;
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(PreviewError::TooLarge);
    }
    let mut reader = ImageReader::new(Cursor::new(data)).with_guessed_format().map_err(|e| PreviewError::Unreadable(e.to_string()))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(w);
    limits.max_image_height = Some(h);
    limits.max_alloc = Some(MAX_PIXELS * 4 + (16 << 20));
    reader.limits(limits);
    reader.decode().map_err(|e| PreviewError::Unreadable(e.to_string()))
}

/// Re-encode for the document: scaled to `max_side`, PNG when it has alpha.
pub(crate) fn encode(image: DynamicImage, max_side: u32) -> Result<(&'static str, Vec<u8>), PreviewError> {
    let image = if image.width().max(image.height()) > max_side {
        image.resize(max_side, max_side, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let mut out = Cursor::new(Vec::new());
    if image.color().has_alpha() {
        image.write_to(&mut out, ImageFormat::Png).map_err(|e| PreviewError::Unreadable(e.to_string()))?;
        Ok(("image/png", out.into_inner()))
    } else {
        let rgb = DynamicImage::ImageRgb8(image.to_rgb8());
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
        encoder.encode_image(&rgb).map_err(|e| PreviewError::Unreadable(e.to_string()))?;
        Ok(("image/jpeg", out.into_inner()))
    }
}

/// Best-effort conversion of an embedded Office picture; `None` when the format
/// is not a raster image we decode (EMF/WMF/SVG) or it breaks a limit.
pub(crate) fn embedded(data: &[u8], max_side: u32) -> Option<(&'static str, Vec<u8>)> {
    encode(decode(data).ok()?, max_side).ok()
}

pub(crate) fn preview(data: &[u8]) -> Result<Output, PreviewError> {
    let image = decode(data)?;
    let (w, h) = (image.width(), image.height());
    let (mime, bytes) = encode(image, MAX_SIDE)?;
    let mut out = Output::default();
    let src = out.add_image(mime, bytes);
    out.html = format!(
        "{}<img class=\"fuu-image\" src=\"{src}\" alt=\"\">",
        crate::html::note(&format!("{w} × {h} ピクセル"))
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32, alpha: bool) -> Vec<u8> {
        let image = if alpha {
            DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 128])))
        } else {
            DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb([10, 20, 30])))
        };
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn small_images_keep_their_size_and_alpha() {
        let out = preview(&png(40, 30, true)).unwrap();
        assert_eq!(out.images.len(), 1);
        assert_eq!(out.images[0].mime, "image/png");
        assert!(out.html.contains("40 × 30"));
        let opaque = preview(&png(40, 30, false)).unwrap();
        assert_eq!(opaque.images[0].mime, "image/jpeg");
    }

    #[test]
    fn large_images_are_scaled_down() {
        let out = preview(&png(3000, 1000, false)).unwrap();
        let back = image::load_from_memory(&out.images[0].data).unwrap();
        assert_eq!((back.width(), back.height()), (2400, 800));
    }

    #[test]
    fn decompression_bombs_are_refused_from_the_header() {
        // A PNG header claiming 60000 × 60000 pixels with no real data.
        let mut bomb = png(1, 1, false);
        bomb[16..20].copy_from_slice(&60000u32.to_be_bytes());
        bomb[20..24].copy_from_slice(&60000u32.to_be_bytes());
        assert!(matches!(preview(&bomb), Err(PreviewError::TooLarge | PreviewError::Unreadable(_))));
        assert!(preview(b"not an image").is_err());
    }
}
