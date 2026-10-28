//! Local image crop and masking. OCR never receives network access or persists pixels.
use image::{
    imageops::{self, FilterType},
    DynamicImage, ImageFormat, Rgba, RgbaImage,
};
use std::{
    io::Cursor,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    pub fn valid(self, min: f64) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
            && self.x >= 0.0
            && self.y >= 0.0
            && self.width >= min
            && self.height >= min
            && self.x + self.width <= 1.000001
            && self.y + self.height <= 1.000001
    }
}
pub struct SanitizedImage {
    pub png: Vec<u8>,
    pub boxes: Vec<Rect>,
    pub automatic_regions: usize,
    pub width: u32,
    pub height: u32,
}
static OCR_BUSY: AtomicBool = AtomicBool::new(false);

/// Crop is normalized against the original image; masks are relative to the crop.
pub fn sanitize(bytes: &[u8], crop: Rect, masks: &[Rect]) -> Result<SanitizedImage, String> {
    if bytes.is_empty()
        || bytes.len() > 10 * 1024 * 1024
        || !crop.valid(0.05)
        || masks.len() > 32
        || masks.iter().any(|r| !r.valid(0.001))
    {
        return Err("Invalid image, crop, or masks.".into());
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "Invalid image.")?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits.clone());
    let (w, h) = reader
        .into_dimensions()
        .map_err(|_| "Invalid image dimensions.")?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 16_000_000 {
        return Err("Image exceeds the 16 megapixel limit.".into());
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "Invalid image.")?;
    reader.limits(limits);
    let raw = reader
        .decode()
        .map_err(|_| "Could not decode image.")?
        .to_rgba8();
    let x = (crop.x * w as f64).floor() as u32;
    let y = (crop.y * h as f64).floor() as u32;
    let right = ((crop.x + crop.width) * w as f64).ceil().min(w as f64) as u32;
    let bottom = ((crop.y + crop.height) * h as f64).ceil().min(h as f64) as u32;
    if right <= x || bottom <= y {
        return Err("Crop is empty.".into());
    }
    let mut image = imageops::crop_imm(&raw, x, y, right - x, bottom - y).to_image();
    for p in image.pixels_mut() {
        let a = p[3] as u32;
        for c in 0..3 {
            p[c] = ((p[c] as u32 * a + [9, 9, 14][c] * (255 - a)) / 255) as u8;
        }
        p[3] = 255;
    }
    if image.width() > 1600 || image.height() > 1600 {
        let scale = (1600.0 / image.width() as f64).min(1600.0 / image.height() as f64);
        image = imageops::resize(
            &image,
            (image.width() as f64 * scale).round().max(1.0) as u32,
            (image.height() as f64 * scale).round().max(1.0) as u32,
            FilterType::Lanczos3,
        );
    }
    let ocr = bounded_ocr(image.clone())?;
    compose(image, crop, masks, ocr)
}
fn compose(
    mut image: RgbaImage,
    _crop: Rect,
    masks: &[Rect],
    ocr: Vec<Rect>,
) -> Result<SanitizedImage, String> {
    let auto = ocr.len();
    // The editor draws the cropped upload; its boxes share those coordinates.
    let boxes = ocr.clone();
    for rect in &ocr {
        paint(&mut image, *rect);
    }
    for rect in masks {
        paint(&mut image, *rect);
    }
    let original_width = image.width();
    let original_height = image.height();
    let scale = (1280.0 / image.width() as f64).min(800.0 / image.height() as f64);
    let w = (image.width() as f64 * scale).round().max(1.0) as u32;
    let h = (image.height() as f64 * scale).round().max(1.0) as u32;
    let image = imageops::resize(&image, w, h, FilterType::Lanczos3);
    let mut canvas = RgbaImage::from_pixel(1280, 800, Rgba([9, 9, 14, 255]));
    imageops::overlay(
        &mut canvas,
        &image,
        ((1280 - w) / 2) as i64,
        ((800 - h) / 2) as i64,
    );
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(canvas)
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|_| "Cannot encode masked image.")?;
    Ok(SanitizedImage {
        png: out.into_inner(),
        boxes,
        automatic_regions: auto,
        width: original_width,
        height: original_height,
    })
}
fn paint(image: &mut RgbaImage, r: Rect) {
    let w = image.width();
    let h = image.height();
    let x = (r.x * w as f64).floor().clamp(0.0, w as f64) as u32;
    let y = (r.y * h as f64).floor().clamp(0.0, h as f64) as u32;
    let right = ((r.x + r.width) * w as f64).ceil().clamp(0.0, w as f64) as u32;
    let bottom = ((r.y + r.height) * h as f64).ceil().clamp(0.0, h as f64) as u32;
    if right <= x || bottom <= y {
        return;
    }
    let mut sum = [0u64; 3];
    for yy in y..bottom {
        for xx in x..right {
            let p = image.get_pixel(xx, yy);
            for c in 0..3 {
                sum[c] += p[c] as u64;
            }
        }
    }
    let n = u64::from(right - x) * u64::from(bottom - y);
    let color = Rgba([
        (sum[0] / n) as u8,
        (sum[1] / n) as u8,
        (sum[2] / n) as u8,
        255,
    ]);
    for yy in y..bottom {
        for xx in x..right {
            image.put_pixel(xx, yy, color);
        }
    }
}
fn bounded_ocr(image: RgbaImage) -> Result<Vec<Rect>, String> {
    if OCR_BUSY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("Local OCR is busy. Try again shortly.".into());
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    if std::thread::Builder::new()
        .name("social-local-ocr".into())
        .spawn(move || {
            struct Guard;
            impl Drop for Guard {
                fn drop(&mut self) {
                    OCR_BUSY.store(false, Ordering::Release);
                }
            }
            let _guard = Guard;
            let _ = tx.send(ocr(image));
        })
        .is_err()
    {
        OCR_BUSY.store(false, Ordering::Release);
        return Err("Cannot start local OCR.".into());
    }
    rx.recv_timeout(Duration::from_secs(20))
        .map_err(|_| "Local OCR timed out.".to_string())?
}
#[cfg(windows)]
fn ocr(image: RgbaImage) -> Result<Vec<Rect>, String> {
    use windows::{
        Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
        Media::Ocr::OcrEngine,
        Storage::Streams::DataWriter,
    };
    let failure = |_| {
        "Windows OCR is unavailable; install a supported OCR language and try again.".to_string()
    };
    let (w, h) = image.dimensions();
    let mut bgra = image.into_raw();
    for p in bgra.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    let writer = DataWriter::new().map_err(failure)?;
    writer.WriteBytes(&bgra).map_err(failure)?;
    let buffer = writer.DetachBuffer().map_err(failure)?;
    let bitmap =
        SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, w as i32, h as i32)
            .map_err(failure)?;
    let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(failure)?;
    let result = engine
        .RecognizeAsync(&bitmap)
        .map_err(failure)?
        .join()
        .map_err(failure)?;
    if result
        .TextAngle()
        .ok()
        .and_then(|a| a.Value().ok())
        .is_some_and(|a| a.abs() > 5.0)
    {
        return Err("The image text is rotated; crop or straighten it before masking.".into());
    }
    let mut boxes = Vec::new();
    for line in result.Lines().map_err(failure)? {
        for word in line.Words().map_err(failure)? {
            let r = word.BoundingRect().map_err(failure)?;
            let x = (r.X as f64 - 6.0).max(0.0);
            let y = (r.Y as f64 - 6.0).max(0.0);
            let right = (r.X as f64 + r.Width as f64 + 6.0).min(w as f64);
            let bottom = (r.Y as f64 + r.Height as f64 + 6.0).min(h as f64);
            if right > x && bottom > y {
                boxes.push(Rect {
                    x: x / w as f64,
                    y: y / h as f64,
                    width: (right - x) / w as f64,
                    height: (bottom - y) / h as f64,
                });
            }
        }
    }
    Ok(boxes)
}
#[cfg(not(windows))]
fn ocr(_: RgbaImage) -> Result<Vec<Rect>, String> {
    Err("Local image OCR is only available on Windows.".into())
}
