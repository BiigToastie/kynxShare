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
            visible: true,
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
    pub fn set_visible(&self, _visible: bool) {}
    pub fn close(&self) {}
}
