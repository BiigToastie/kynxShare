use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FramePixelFormat {
    Bgra8,
}

#[derive(Debug, Clone)]
pub struct CapturedFrame {
    pub monitor_id: String,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: FramePixelFormat,
    /// Shared pixel buffer — cheap to clone across threads.
    pub pixels: Arc<[u8]>,
    pub timestamp_ms: u64,
}

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("DXGI / graphics error: {0}")]
    Graphics(String),
    #[error("monitor not found: {0}")]
    MonitorNotFound(String),
    #[error("capture timeout")]
    Timeout,
    #[error("access lost — recreate duplication (display mode change)")]
    AccessLost,
    #[error("platform not supported")]
    UnsupportedPlatform,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
