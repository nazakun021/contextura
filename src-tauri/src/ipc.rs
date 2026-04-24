//! IPC payload types for `translation-update`, `translation-error`, and
//! `translation-started` events emitted from Rust to the `WebView` frontend.
//!
//! These structs are serialised via `serde` across the IPC boundary and are
//! consumed by `overlay.js`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TranslationBox {
    pub id: String,
    pub translated: String,
    pub original: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub is_vertical: bool,
    pub bg_color: String,
    pub fg_color: String,
    pub confidence: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TranslationPayload {
    pub boxes: Vec<TranslationBox>,
    pub scale_factor: f32,
    pub display_id: u32,
    pub frame_id: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TranslationErrorPayload {
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TranslationStartedPayload {
    pub display_id: u32,
}
