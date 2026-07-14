//! Output backends: share window, virtual camera shared memory, VDD helpers.

mod share_window;
mod vdd;
mod virtual_cam;

pub use share_window::{ShareWindow, ShareWindowConfig};
pub use vdd::{
    detect_virtual_display_driver, install_driver_guidance, is_virtual_monitor,
    open_driver_installer, VirtualDisplaySession, VirtualDisplayStatus,
};
pub use virtual_cam::{VirtualCamera, VirtualCameraConfig};
