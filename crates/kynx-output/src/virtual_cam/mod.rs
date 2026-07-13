#[cfg(windows)]
mod win;

use serde::{Deserialize, Serialize};

/// Named shared-memory mapping for the kynxShare virtual camera bridge.
pub const VIRTUAL_CAM_MAPPING_NAME: &str = "Local\\KynxShareVirtualCam";

/// Header laid out at the start of the shared mapping (little-endian).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtualCamHeader {
    pub magic: u32,
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: u32,
    pub frame_id: u64,
    pub timestamp_ms: u64,
    pub data_offset: u32,
    pub data_size: u32,
}

pub const VIRTUAL_CAM_MAGIC: u32 = 0x584E594B; // 'KYNX' LE
pub const VIRTUAL_CAM_HEADER_SIZE: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualCameraConfig {
    pub enabled: bool,
    pub max_width: u32,
    pub max_height: u32,
}

impl Default for VirtualCameraConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_width: 3840,
            max_height: 2160,
        }
    }
}

#[cfg(windows)]
pub use win::VirtualCamera;

#[cfg(not(windows))]
pub struct VirtualCamera;

#[cfg(not(windows))]
impl VirtualCamera {
    pub fn open(_cfg: VirtualCameraConfig) -> anyhow::Result<Self> {
        Ok(Self)
    }
    pub fn push_frame(&self, _width: u32, _height: u32, _bgra: &[u8], _timestamp_ms: u64) {}
    pub fn set_enabled(&self, _enabled: bool) {}
    pub fn close(&self) {}
}
