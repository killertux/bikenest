//! Image processing (M4): decode → apply EXIF orientation → re-encode JPEG →
//! thumbnail. The raw upload is never stored; only these derivatives are.
//!
//! Decoding and the two JPEG encodes are pure CPU work on images of up to 20
//! megapixels, so they run on `tokio::task::spawn_blocking` — inline, a handful
//! of concurrent uploads would occupy every runtime worker and stall the whole
//! server, `/healthz` included. A [`Semaphore`] sized to the machine's
//! parallelism bounds how many run at once, so an upload burst *queues* instead
//! of spawning an unbounded number of blocking tasks (each of which allocates
//! several times the decoded image).

use async_trait::async_trait;
use bikenest_application::{PhotoError, ProcessedImage};
use bikenest_domain::{PhotoDimensions, PhotoLimits};
use image::codecs::jpeg::JpegEncoder;
use image::imageops;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use std::io::Cursor;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// [`ImageProcessor`](bikenest_application::ImageProcessor) via the `image`
/// crate. Deterministic, pure-Rust decode/re-encode for jpeg/png/webp inputs.
pub struct LocalImageProcessor {
    limits: PhotoLimits,
    /// Concurrency budget for the blocking decode/encode work.
    permits: Arc<Semaphore>,
}

impl LocalImageProcessor {
    pub fn new(limits: PhotoLimits) -> Self {
        let parallelism = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .max(1);
        Self {
            limits,
            permits: Arc::new(Semaphore::new(parallelism)),
        }
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

    /// The whole pipeline, synchronously. Called only from a blocking task.
    fn process_blocking(limits: PhotoLimits, bytes: &[u8]) -> Result<ProcessedImage, PhotoError> {
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
        if !dims.within_limit(limits.max_megapixels) {
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
            let mut enc = JpegEncoder::new_with_quality(&mut full, limits.derivative_quality);
            enc.encode_image(&oriented)
                .map_err(|_| PhotoError::Undecodable)?;
        }

        // Thumbnail: longest side ≤ configured side, aspect preserved.
        let thumb_rgb = DynamicImage::ImageRgb8(oriented)
            .thumbnail(limits.thumb_max_side, limits.thumb_max_side)
            .to_rgb8();
        let mut thumb = Vec::new();
        {
            let mut enc = JpegEncoder::new_with_quality(&mut thumb, limits.derivative_quality);
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

#[async_trait]
impl bikenest_application::ImageProcessor for LocalImageProcessor {
    async fn process(&self, bytes: &[u8]) -> Result<ProcessedImage, PhotoError> {
        // Wait for a slot *before* copying the upload onto a blocking thread,
        // so a burst queues here — holding nothing but the caller's own buffer
        // — instead of running N decodes at once. The semaphore is never
        // closed, so `acquire_owned` only fails if it were.
        let _permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| PhotoError::Undecodable)?;
        let limits = self.limits;
        let bytes = bytes.to_vec();
        tokio::task::spawn_blocking(move || Self::process_blocking(limits, &bytes))
            .await
            // A JoinError means the blocking pool panicked or is shutting down;
            // to the caller that is the same as an unusable upload.
            .map_err(|_| PhotoError::Undecodable)?
    }
}
