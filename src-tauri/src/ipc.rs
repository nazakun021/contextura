//! IPC payload types for `translation-update`, `translation-error`, and
//! `translation-started` events emitted from Rust to the `WebView` frontend.
//!
//! These structs are serialised via `serde` across the IPC boundary and are
//! consumed by `overlay.js`. They will be constructed by the engine emitter
//! once the capture→OCR→translate pipeline is integrated.
// These structs are all `pub` — Rust does not fire `dead_code` on pub items,
// so #[allow] is used here rather than #[expect] (which would fire a warning
// when the lint is never triggered). They will be constructed by the engine
// emitter once the pipeline is integrated.
#![allow(dead_code)]

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
