//! Image-byte helpers shared by the pipeline and bundle import.
//!
//! The bundle contract fixes the filename at `image.png`. Providers (and
//! loose imports) often hand us JPEG/WebP/etc., so we re-encode when needed.

/// Whether `bytes` are already a PNG.
///
/// [`png_reencode`] answers `None` both for "already PNG" and for "could not
/// convert", which is fine where a failed re-encode is non-fatal. A caller
/// that must refuse undecodable bytes needs to tell the two apart.
pub(crate) fn is_png(bytes: &[u8]) -> bool {
    matches!(image::guess_format(bytes), Ok(image::ImageFormat::Png))
}

/// PNG bytes for `bytes`, or `None` when they are not a decodable image.
///
/// The strict counterpart to [`png_reencode`], for import and attach: there is
/// no paid generation to salvage there — the user handed us a file and can hand
/// us another — so a still we cannot turn into a real `image.png` is refused
/// rather than written as an unreadable one. The pipeline keeps the lenient
/// path, where discarding a generation costs money.
pub(crate) fn to_png(bytes: Vec<u8>) -> Option<Vec<u8>> {
    if let Some(png) = png_reencode(&bytes) {
        return Some(png);
    }
    is_png(&bytes).then_some(bytes)
}

/// Re-encode image bytes to PNG when they aren't already.
///
/// Two things assume PNG: the bundle filename `image.png`, and the data-URI
/// fallback for providers without an upload endpoint, which hardcodes
/// `data:image/png;base64,`.
///
/// Returns `None` when the bytes are already PNG (the common case, no work) or
/// when they can't be converted. A failed re-encode is non-fatal: keeping a
/// usable image in the wrong container beats discarding the file.
pub(crate) fn png_reencode(bytes: &[u8]) -> Option<Vec<u8>> {
    let format = match image::guess_format(bytes) {
        Ok(image::ImageFormat::Png) => return None,
        Ok(format) => format,
        Err(e) => {
            tracing::warn!("Unrecognized image format, writing bytes as-is: {e}");
            return None;
        }
    };

    let decoded = match image::load_from_memory_with_format(bytes, format) {
        Ok(img) => img,
        Err(e) => {
            tracing::warn!("Could not decode {format:?} image for PNG re-encode: {e}");
            return None;
        }
    };

    let mut out = std::io::Cursor::new(Vec::new());
    match decoded.write_to(&mut out, image::ImageFormat::Png) {
        Ok(()) => {
            tracing::info!("Re-encoded {format:?} image to PNG");
            Some(out.into_inner())
        }
        Err(e) => {
            tracing::warn!("Could not encode image as PNG: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_png, png_reencode, to_png};

    fn encode_sample(format: image::ImageFormat) -> Vec<u8> {
        let img = image::DynamicImage::new_rgb8(2, 2);
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    #[test]
    fn png_bytes_are_left_alone() {
        assert!(png_reencode(&encode_sample(image::ImageFormat::Png)).is_none());
    }

    #[test]
    fn jpeg_bytes_are_reencoded_to_png() {
        let jpeg = encode_sample(image::ImageFormat::Jpeg);
        let png = png_reencode(&jpeg).expect("JPEG must be re-encoded");
        assert_eq!(
            image::guess_format(&png).unwrap(),
            image::ImageFormat::Png,
            "re-encoded bytes are not PNG"
        );
    }

    #[test]
    fn undecodable_bytes_are_kept_as_is() {
        assert!(png_reencode(b"not an image at all").is_none());
    }

    #[test]
    fn to_png_accepts_real_images_and_refuses_junk() {
        let png = encode_sample(image::ImageFormat::Png);
        assert_eq!(
            to_png(png.clone()),
            Some(png),
            "already-PNG bytes pass through untouched"
        );
        let jpeg = encode_sample(image::ImageFormat::Jpeg);
        let converted = to_png(jpeg).expect("JPEG converts");
        assert_eq!(
            image::guess_format(&converted).unwrap(),
            image::ImageFormat::Png
        );
        assert_eq!(
            to_png(b"not an image at all".to_vec()),
            None,
            "junk must not become image.png"
        );
    }

    #[test]
    fn is_png_separates_already_png_from_undecodable() {
        // Both answer `None` from png_reencode; only one is a usable image.
        assert!(is_png(&encode_sample(image::ImageFormat::Png)));
        assert!(!is_png(&encode_sample(image::ImageFormat::Jpeg)));
        assert!(!is_png(b"not an image at all"));
    }
}
