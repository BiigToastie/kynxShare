//! Multi-monitor capture via DXGI Desktop Duplication (Windows).

mod monitor;
mod types;

#[cfg(windows)]
mod dxgi;

pub use monitor::{enumerate_monitors, MonitorInfo};
pub use types::{CaptureError, CapturedFrame, FramePixelFormat};

#[cfg(windows)]
pub use dxgi::{CaptureSession, MultiCapture};

#[cfg(not(windows))]
mod stub;

#[cfg(not(windows))]
pub use stub::{CaptureSession, MultiCapture};
