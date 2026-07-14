use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    /// Stable id from device name + desktop origin
    pub id: String,
    pub name: String,
    pub device_name: String,
    pub adapter_index: u32,
    pub output_index: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    pub is_primary: bool,
    pub scale_percent: u32,
}

#[cfg(windows)]
pub fn enumerate_monitors() -> anyhow::Result<Vec<MonitorInfo>> {
    crate::dxgi::enumerate_monitors_dxgi()
}

#[cfg(not(windows))]
pub fn enumerate_monitors() -> anyhow::Result<Vec<MonitorInfo>> {
    Ok(vec![MonitorInfo {
        id: "stub-0".into(),
        name: "Stub Display".into(),
        device_name: "\\\\.\\DISPLAY1".into(),
        adapter_index: 0,
        output_index: 0,
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
        refresh_hz: 60,
        is_primary: true,
        scale_percent: 100,
    }])
}
