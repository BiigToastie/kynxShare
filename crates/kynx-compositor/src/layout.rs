use kynx_capture::MonitorInfo;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    #[default]
    StaticLayout,
    MouseFollow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorPlacement {
    pub monitor_id: String,
    pub enabled: bool,
    /// Position on the virtual canvas (pixels)
    pub x: i32,
    pub y: i32,
    /// Scale relative to native size (1.0 = native)
    pub scale: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutConfig {
    pub placements: Vec<MonitorPlacement>,
    /// Output canvas size. None = auto from placements.
    pub canvas_width: Option<u32>,
    pub canvas_height: Option<u32>,
    /// Max dimension clamp for Discord-friendly downscale (0 = no clamp)
    pub max_width: u32,
    pub max_height: u32,
    pub mode: OutputMode,
    pub follow: crate::mouse_follow::MouseFollowConfig,
    pub background_bgra: [u8; 4],
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            placements: Vec::new(),
            canvas_width: None,
            canvas_height: None,
            max_width: 7680,
            max_height: 4320,
            mode: OutputMode::StaticLayout,
            follow: crate::mouse_follow::MouseFollowConfig::default(),
            background_bgra: [18, 18, 20, 255],
        }
    }
}

impl LayoutConfig {
    pub fn from_monitors(monitors: &[MonitorInfo]) -> Self {
        let mut cfg = Self::default();
        cfg.placements = auto_layout_side_by_side(monitors);
        let (w, h) = compute_native_canvas_size(&cfg.placements, monitors);
        cfg.canvas_width = Some(w);
        cfg.canvas_height = Some(h);
        cfg
    }

    pub fn resolve_canvas_size(&self, monitors: &[MonitorInfo]) -> (u32, u32) {
        let (native_w, native_h) = match (self.canvas_width, self.canvas_height) {
            (Some(w), Some(h)) => (w.max(1), h.max(1)),
            _ => compute_native_canvas_size(&self.placements, monitors),
        };

        let max_w = if self.max_width == 0 {
            u32::MAX
        } else {
            self.max_width
        };
        let max_h = if self.max_height == 0 {
            u32::MAX
        } else {
            self.max_height
        };

        // Uniform fit into max box — never squash aspect ratio
        let scale_w = max_w as f64 / native_w as f64;
        let scale_h = max_h as f64 / native_h as f64;
        let scale = scale_w.min(scale_h).min(1.0);
        let w = ((native_w as f64) * scale).round().max(1.0) as u32;
        let h = ((native_h as f64) * scale).round().max(1.0) as u32;
        (w, h)
    }
}

/// Place enabled monitors left-to-right by native order (desktop x).
pub fn auto_layout_side_by_side(monitors: &[MonitorInfo]) -> Vec<MonitorPlacement> {
    let mut sorted = monitors.to_vec();
    sorted.sort_by_key(|m| (m.y, m.x));
    let mut x_cursor = 0i32;
    let mut placements = Vec::new();
    for m in sorted {
        placements.push(MonitorPlacement {
            monitor_id: m.id.clone(),
            enabled: true,
            x: x_cursor,
            y: 0,
            scale: 1.0,
        });
        x_cursor += m.width as i32;
    }
    placements
}

pub fn compute_native_canvas_size(
    placements: &[MonitorPlacement],
    monitors: &[MonitorInfo],
) -> (u32, u32) {
    let mut max_r = 1i32;
    let mut max_b = 1i32;
    for p in placements.iter().filter(|p| p.enabled) {
        let Some(m) = monitors.iter().find(|m| m.id == p.monitor_id) else {
            continue;
        };
        let w = (m.width as f32 * p.scale).round() as i32;
        let h = (m.height as f32 * p.scale).round() as i32;
        max_r = max_r.max(p.x + w);
        max_b = max_b.max(p.y + h);
    }
    (max_r.max(1) as u32, max_b.max(1) as u32)
}

/// Snap placement to a grid (e.g. 16px).
pub fn snap_placement(p: &mut MonitorPlacement, grid: i32) {
    if grid <= 1 {
        return;
    }
    p.x = ((p.x as f32 / grid as f32).round() as i32) * grid;
    p.y = ((p.y as f32 / grid as f32).round() as i32) * grid;
}

/// Add newly detected monitors and drop removed ones. Recompute auto canvas size.
pub fn sync_layout_with_monitors(layout: &mut LayoutConfig, monitors: &[MonitorInfo]) {
    let existing: std::collections::HashSet<String> =
        layout.placements.iter().map(|p| p.monitor_id.clone()).collect();

    let mut x_cursor = layout
        .placements
        .iter()
        .filter(|p| p.enabled)
        .filter_map(|p| {
            monitors
                .iter()
                .find(|m| m.id == p.monitor_id)
                .map(|m| p.x + (m.width as f32 * p.scale).round() as i32)
        })
        .max()
        .unwrap_or(0);

    for m in monitors {
        if existing.contains(&m.id) {
            continue;
        }
        // Also try match by device name for id migrations
        if layout
            .placements
            .iter()
            .any(|p| p.monitor_id.contains(&m.device_name))
        {
            // Remap old id → new stable id
            if let Some(p) = layout
                .placements
                .iter_mut()
                .find(|p| p.monitor_id.contains(&m.device_name) && p.monitor_id != m.id)
            {
                p.monitor_id = m.id.clone();
            }
            continue;
        }
        layout.placements.push(MonitorPlacement {
            monitor_id: m.id.clone(),
            enabled: true,
            x: x_cursor,
            y: 0,
            scale: 1.0,
        });
        x_cursor += m.width as i32;
    }

    let ids: std::collections::HashSet<&str> = monitors.iter().map(|m| m.id.as_str()).collect();
    layout
        .placements
        .retain(|p| ids.contains(p.monitor_id.as_str()) || monitors.iter().any(|m| p.monitor_id.contains(&m.device_name)));

    let (w, h) = compute_native_canvas_size(&layout.placements, monitors);
    layout.canvas_width = Some(w);
    layout.canvas_height = Some(h);
}
