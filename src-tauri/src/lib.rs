mod image_handler;

use image_handler::{ImageCache, ImageInfo};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
async fn open_image(path: String, cache: State<'_, ImageCache>) -> Result<ImageInfo, String> {
    image_handler::open_image(path, cache.0.clone()).await
}

#[tauri::command]
async fn get_tile(path: String, x: u32, y: u32, w: u32, h: u32, scale: f64, cache: State<'_, ImageCache>) -> Result<String, String> {
    image_handler::get_tile(path, x, y, w, h, scale, cache.0.clone()).await
}

#[tauri::command]
async fn get_thumbnail(path: String, max_size: u32, cache: State<'_, ImageCache>) -> Result<String, String> {
    image_handler::get_thumbnail(path, max_size, cache.0.clone()).await
}

#[tauri::command]
async fn get_region_stats(path: String, x: u32, y: u32, w: u32, h: u32, cache: State<'_, ImageCache>) -> Result<f64, String> {
    image_handler::get_region_stats(path, x, y, w, h, cache.0.clone()).await
}

#[tauri::command]
fn list_images(folder: String) -> Result<Vec<String>, String> {
    image_handler::list_images(&folder)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(ImageCache(Arc::new(tokio::sync::RwLock::new(Vec::new()))))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            open_image,
            get_tile,
            get_thumbnail,
            get_region_stats,
            list_images,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
