//! Output backends: share window, virtual camera shared memory, VDD helpers.

mod share_window;
mod virtual_cam;
mod vdd;

pub use share_window::{ShareWindow, ShareWindowConfig};
pub use vdd::{detect_virtual_display_driver, VirtualDisplayStatus};
pub use virtual_cam::{VirtualCamera, VirtualCameraConfig, VIRTUAL_CAM_MAPPING_NAME};
