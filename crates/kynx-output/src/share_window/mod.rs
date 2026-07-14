mod win;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareWindowConfig {
    pub title: String,
    pub always_on_top: bool,
    pub visible: bool,
}

impl Default for ShareWindowConfig {
    fn default() -> Self {
        Self {
            title: "kynxShare Output".into(),
            always_on_top: false,
            visible: false,
        }
    }
}

#[cfg(windows)]
pub use win::ShareWindow;

#[cfg(not(windows))]
pub struct ShareWindow;

#[cfg(not(windows))]
impl ShareWindow {
    pub fn create(_cfg: ShareWindowConfig) -> anyhow::Result<Self> {
        Ok(Self)
    }
    pub fn present(&self, _width: u32, _height: u32, _bgra: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }
    pub fn present_arc(
        &self,
        _width: u32,
        _height: u32,
        _pixels: std::sync::Arc<[u8]>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
    pub fn place_on_monitor(&self, _x: i32, _y: i32, _width: u32, _height: u32) {}
    pub fn set_visible(&self, _visible: bool) {}
    pub fn show_for_capture(&self) {}
    pub fn close(&self) {}
}
