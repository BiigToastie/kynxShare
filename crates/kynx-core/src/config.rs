use directories::ProjectDirs;
use kynx_compositor::{LayoutConfig, OutputMode};
use kynx_output::{ShareWindowConfig, VirtualCameraConfig};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputChannels {
    pub share_window: bool,
    pub virtual_camera: bool,
    pub always_on_top: bool,
    /// Show the Discord capture window on screen (off by default — stays hidden but capturable when shown)
    #[serde(default)]
    pub show_share_window: bool,
    /// Plug a Parsec virtual monitor so Discord lists it under Bildschirm/Screen.
    #[serde(default)]
    pub virtual_display: bool,
    /// Encode JPEG previews for the UI. Disable while streaming to save CPU.
    #[serde(default = "default_true")]
    pub ui_live_preview: bool,
}

fn default_true() -> bool {
    true
}

impl Default for OutputChannels {
    fn default() -> Self {
        Self {
            share_window: true,
            virtual_camera: false,
            always_on_top: false,
            show_share_window: false,
            virtual_display: true,
            ui_live_preview: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub layout: LayoutConfig,
    pub outputs: OutputChannels,
    pub share_window: ShareWindowConfig,
    pub virtual_camera: VirtualCameraConfig,
    pub target_fps: u32,
    pub start_with_windows: bool,
    pub onboarding_done: bool,
    pub selected_monitor_ids: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            layout: LayoutConfig::default(),
            outputs: OutputChannels::default(),
            share_window: ShareWindowConfig::default(),
            virtual_camera: VirtualCameraConfig::default(),
            target_fps: 60,
            start_with_windows: false,
            onboarding_done: false,
            selected_monitor_ids: Vec::new(),
        }
    }
}

impl AppConfig {
    pub fn config_dir() -> anyhow::Result<PathBuf> {
        let dirs = ProjectDirs::from("app", "kynxShare", "kynxShare")
            .ok_or_else(|| anyhow::anyhow!("could not resolve config directory"))?;
        let path = dirs.config_dir().to_path_buf();
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    pub fn config_path() -> anyhow::Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.json"))
    }

    pub fn load() -> anyhow::Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            let cfg = Self::default();
            cfg.save()?;
            return Ok(cfg);
        }
        let data = fs::read_to_string(&path)?;
        let cfg: Self = serde_json::from_str(&data)?;
        Ok(cfg)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path()?;
        let data = serde_json::to_string_pretty(self)?;
        fs::write(path, data)?;
        Ok(())
    }

    pub fn set_mode(&mut self, mode: OutputMode) {
        self.layout.mode = mode;
    }
}
