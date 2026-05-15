use base64::{engine::general_purpose, Engine};
use fast_image_resize::{images::Image, images::ImageRef, PixelType, Resizer};
use image::{codecs::jpeg::JpegEncoder, DynamicImage, GenericImageView, RgbImage};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::sync::Arc;
use tokio::sync::RwLock;
use walkdir::WalkDir;

// Vec with max 2 entries — lets primary + pair image coexist in cache.
// Arc<RgbImage>: O(1) clone, lock released immediately; 25% less memory than RGBA.
pub struct ImageCache(pub Arc<RwLock<Vec<(String, Arc<RgbImage>)>>>);

#[derive(Serialize, Deserialize)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub path: String,
    pub exif: Vec<ExifEntry>,
}

#[derive(Serialize, Deserialize)]
pub struct ExifEntry {
    pub tag: String,
    pub value: String,
}

static IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "tiff", "tif", "bmp", "gif",
];

pub async fn open_image(
    path: String,
    cache: Arc<RwLock<Vec<(String, Arc<RgbImage>)>>>,
) -> Result<ImageInfo, String> {
    let p1 = path.clone();
    let p2 = path.clone();

    // Read dimensions and EXIF in parallel — saves ~50-100 ms vs sequential.
    let (dim_res, exif_res) = tokio::join!(
        tauri::async_runtime::spawn_blocking(move || {
            image::ImageReader::open(&p1)
                .map_err(|e| e.to_string())?
                .with_guessed_format()
                .map_err(|e| e.to_string())?
                .into_dimensions()
                .map_err(|e| e.to_string())
        }),
        tauri::async_runtime::spawn_blocking(move || read_exif(&p2))
    );

    let (width, height) = dim_res.map_err(|e| e.to_string())??;
    let exif = exif_res.unwrap_or_default();

    // Kick off full decode immediately so cache is warm by first tile request.
    let cache2 = cache.clone();
    let path2  = path.clone();
    tauri::async_runtime::spawn(async move {
        let _ = ensure_cached(&path2, cache2).await;
    });

    Ok(ImageInfo { width, height, path, exif })
}

async fn ensure_cached(
    path: &str,
    cache: Arc<RwLock<Vec<(String, Arc<RgbImage>)>>>,
) -> Result<(), String> {
    if cache.read().await.iter().any(|(p, _)| p == path) {
        return Ok(());
    }
    let mut w = cache.write().await;
    if w.iter().any(|(p, _)| p == path) {
        return Ok(());
    }
    let p = path.to_string();
    let img = tauri::async_runtime::spawn_blocking(move || {
        decode_image(&p).map(|i| Arc::new(i.into_rgb8()))
    })
    .await
    .map_err(|e| e.to_string())??;

    w.insert(0, (path.to_string(), img));
    w.truncate(2);
    Ok(())
}

pub async fn get_tile(
    path: String,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    scale: f64,
    cache: Arc<RwLock<Vec<(String, Arc<RgbImage>)>>>,
) -> Result<String, String> {
    ensure_cached(&path, cache.clone()).await?;

    // Clone the Arc — lock released before any CPU work runs.
    let img = {
        let lock = cache.read().await;
        lock.iter()
            .find(|(p, _)| p == &path)
            .map(|(_, i)| Arc::clone(i))
            .ok_or_else(|| "image not in cache".to_string())?
    };

    let (img_w, img_h) = img.dimensions();
    let x = x.min(img_w.saturating_sub(1));
    let y = y.min(img_h.saturating_sub(1));
    let w = w.min(img_w - x);
    let h = h.min(img_h - y);
    let out_w = ((w as f64 * scale) as u32).max(1);
    let out_h = ((h as f64 * scale) as u32).max(1);

    tauri::async_runtime::spawn_blocking(move || {
        let cropped = image::imageops::crop_imm(&*img, x, y, w, h).to_image();
        let output  = if (scale - 1.0).abs() < 0.001 { cropped } else { resize_rgb(&cropped, out_w, out_h)? };
        let mut buf = Cursor::new(Vec::with_capacity(out_w as usize * out_h as usize / 4));
        output
            .write_with_encoder(JpegEncoder::new_with_quality(&mut buf, 75))
            .map_err(|e| e.to_string())?;
        Ok::<String, String>(general_purpose::STANDARD.encode(buf.into_inner()))
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn get_thumbnail(
    path: String,
    max_size: u32,
    cache: Arc<RwLock<Vec<(String, Arc<RgbImage>)>>>,
) -> Result<String, String> {
    ensure_cached(&path, cache.clone()).await?;
    let img = {
        let lock = cache.read().await;
        lock.iter()
            .find(|(p, _)| p == &path)
            .map(|(_, i)| Arc::clone(i))
            .ok_or_else(|| "image not in cache".to_string())?
    };
    tauri::async_runtime::spawn_blocking(move || encode_thumbnail(&img, max_size))
        .await
        .map_err(|e| e.to_string())?
}

pub async fn get_region_stats(
    path: String,
    x: u32, y: u32, w: u32, h: u32,
    cache: Arc<RwLock<Vec<(String, Arc<RgbImage>)>>>,
) -> Result<f64, String> {
    ensure_cached(&path, cache.clone()).await?;
    let img = {
        let lock = cache.read().await;
        lock.iter()
            .find(|(p, _)| p == &path)
            .map(|(_, i)| Arc::clone(i))
            .ok_or_else(|| "not in cache".to_string())?
    };
    tauri::async_runtime::spawn_blocking(move || {
        let (iw, ih) = img.dimensions();
        let x = x.min(iw.saturating_sub(1));
        let y = y.min(ih.saturating_sub(1));
        let w = w.min(iw - x);
        let h = h.min(ih - y);
        if w == 0 || h == 0 { return Err("empty region".to_string()); }
        let mut sum = 0.0f64;
        for py in y..y + h {
            for px in x..x + w {
                let p = img.get_pixel(px, py);
                sum += 0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64;
            }
        }
        Ok(sum / (w as f64 * h as f64))
    })
    .await
    .map_err(|e| e.to_string())?
}

pub fn list_images(folder: &str) -> Result<Vec<String>, String> {
    let mut paths = Vec::new();
    for entry in WalkDir::new(folder).max_depth(1).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
                    paths.push(entry.path().to_string_lossy().to_string());
                }
            }
        }
    }
    paths.sort();
    Ok(paths)
}

// zune-jpeg is ~2× faster than jpeg-decoder (the image crate's default backend).
// Falls back to the image crate for non-JPEG formats.
fn decode_image(path: &str) -> Result<DynamicImage, String> {
    let lower = path.to_lowercase();
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        if let Some(img) = try_zune_jpeg(path) {
            return Ok(DynamicImage::ImageRgb8(img));
        }
    }
    image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())
}

fn try_zune_jpeg(path: &str) -> Option<RgbImage> {
    let data = std::fs::read(path).ok()?;
    let mut dec = zune_jpeg::JpegDecoder::new(&data);
    dec.decode_headers().ok()?;
    let info = dec.info()?;
    let w = info.width as u32;
    let h = info.height as u32;
    let pixels = dec.decode().ok()?;
    if pixels.len() != w as usize * h as usize * 3 {
        return None; // unexpected size — let image crate handle it
    }
    RgbImage::from_raw(w, h, pixels)
}

// U8x3 — 25% less data than RGBA for fast_image_resize to process.
fn resize_rgb(img: &RgbImage, out_w: u32, out_h: u32) -> Result<RgbImage, String> {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || out_w == 0 || out_h == 0 {
        return Ok(img.clone());
    }
    let src = ImageRef::new(w, h, img.as_raw(), PixelType::U8x3)
        .map_err(|e| e.to_string())?;
    let mut dst = Image::new(out_w, out_h, PixelType::U8x3);
    Resizer::new()
        .resize(
            &src, &mut dst,
            &fast_image_resize::ResizeOptions::new()
                .resize_alg(fast_image_resize::ResizeAlg::Convolution(
                    fast_image_resize::FilterType::Bilinear,
                )),
        )
        .map_err(|e| e.to_string())?;
    RgbImage::from_raw(out_w, out_h, dst.into_vec())
        .ok_or_else(|| "resize buffer too small".to_string())
}

fn encode_thumbnail(img: &RgbImage, max_size: u32) -> Result<String, String> {
    let (w, h) = img.dimensions();
    let (tw, th) = if w > h {
        (max_size, (h * max_size / w).max(1))
    } else {
        ((w * max_size / h).max(1), max_size)
    };
    let thumb = if w > max_size || h > max_size { resize_rgb(img, tw, th)? } else { img.clone() };
    let mut buf = Cursor::new(Vec::new());
    thumb
        .write_with_encoder(JpegEncoder::new_with_quality(&mut buf, 85))
        .map_err(|e| e.to_string())?;
    Ok(general_purpose::STANDARD.encode(buf.into_inner()))
}

fn read_exif(path: &str) -> Vec<ExifEntry> {
    let mut entries = Vec::new();
    if let Ok(f) = std::fs::File::open(path) {
        let mut buf = std::io::BufReader::new(f);
        if let Ok(exif) = exif::Reader::new().read_from_container(&mut buf) {
            for field in exif.fields() {
                entries.push(ExifEntry {
                    tag: field.tag.to_string(),
                    value: field.display_value().to_string(),
                });
            }
        }
    }
    entries
}
