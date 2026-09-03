//! Image processing (M4): decode → apply EXIF orientation → re-encode JPEG →
//! thumbnail. The raw upload is never stored (§80); only these derivatives are.

use async_trait::async_trait;
use bikenest_application::{PhotoError, ProcessedImage};
use bikenest_domain::{PhotoDimensions, PhotoLimits};
use image::codecs::jpeg::JpegEncoder;
use image::imageops;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use std::io::Cursor;

/// [`ImageProcessor`](bikenest_application::ImageProcessor) via the `image`
/// crate. Deterministic, pure-Rust decode/re-encode for jpeg/png/webp inputs.
pub struct LocalImageProcessor {
    limits: PhotoLimits,
}

impl LocalImageProcessor {
    pub fn new(limits: PhotoLimits) -> Self {
        Self { limits }
    }

    fn is_allowed(format: ImageFormat) -> bool {
        matches!(
            format,
            ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP
        )
    }

    /// Map an EXIF orientation to the matching geometric transform.
    fn apply_orientation(rgb: image::RgbImage, orientation: Orientation) -> image::RgbImage {
        use Orientation::*;
        match orientation {
            NoTransforms => rgb,
            Rotate90 => imageops::rotate90(&rgb),
            Rotate180 => imageops::rotate180(&rgb),
            Rotate270 => imageops::rotate270(&rgb),
            FlipHorizontal => imageops::flip_horizontal(&rgb),
            FlipVertical => imageops::flip_vertical(&rgb),
            Rotate90FlipH => imageops::flip_horizontal(&imageops::rotate90(&rgb)),
            Rotate270FlipH => imageops::flip_horizontal(&imageops::rotate270(&rgb)),
        }
    }
}

#[async_trait]
impl bikenest_application::ImageProcessor for LocalImageProcessor {
    async fn process(&self, bytes: &[u8]) -> Result<ProcessedImage, PhotoError> {
        // Content sniff + cheap header read for the 20 MP cap (bomb defense).
        let reader = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|_| PhotoError::Undecodable)?;

        if let Some(format) = reader.format()
            && !Self::is_allowed(format)
        {
            return Err(PhotoError::UnsupportedFormat);
        }

        let mut decoder = reader.into_decoder().map_err(|_| PhotoError::Undecodable)?;
        let (raw_w, raw_h) = decoder.dimensions();
        let dims = PhotoDimensions {
            width: raw_w,
            height: raw_h,
        };
        if !dims.within_limit(self.limits.max_megapixels) {
            return Err(PhotoError::TooManyPixels);
        }
        let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);

        // Decode, then flatten to RGB8 so every input color type encodes uniformly.
        let decoded = DynamicImage::from_decoder(decoder).map_err(|_| PhotoError::Undecodable)?;
        let rgb = decoded.to_rgb8();
        let oriented = Self::apply_orientation(rgb, orientation);
        let (width, height) = oriented.dimensions();

        // Full derivative: JPEG quality 85, no metadata (EXIF/ICC/XMP never
        // written). Constructing pixels from scratch guarantees the strip.
        let mut full = Vec::new();
        {
            let mut enc = JpegEncoder::new_with_quality(&mut full, self.limits.derivative_quality);
            enc.encode_image(&oriented)
                .map_err(|_| PhotoError::Undecodable)?;
        }

        // Thumbnail: longest side ≤ configured side, aspect preserved.
        let thumb_rgb = DynamicImage::ImageRgb8(oriented)
            .thumbnail(self.limits.thumb_max_side, self.limits.thumb_max_side)
            .to_rgb8();
        let mut thumb = Vec::new();
        {
            let mut enc = JpegEncoder::new_with_quality(&mut thumb, self.limits.derivative_quality);
            enc.encode_image(&thumb_rgb)
                .map_err(|_| PhotoError::Undecodable)?;
        }

        Ok(ProcessedImage {
            full,
            thumb,
            dimensions: PhotoDimensions { width, height },
            content_type: "image/jpeg",
        })
    }
}
