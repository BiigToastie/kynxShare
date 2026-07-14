use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseFollowConfig {
    /// Viewport width in output pixels (derived from radius when radius > 0)
    pub width: u32,
    /// Viewport height in output pixels
    pub height: u32,
    /// Soft edge padding — cursor stays this many px from viewport edge when possible
    pub edge_padding: u32,
    /// Lerp factor per frame (0..1). Higher = snappier.
    pub smoothing: f32,
    /// Radius around the cursor in canvas pixels (half of the follow window width).
    /// When > 0, width/height are derived as 16:9 from this radius.
    #[serde(default = "default_radius")]
    pub radius: u32,
}

fn default_radius() -> u32 {
    960
}

impl Default for MouseFollowConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            edge_padding: 120,
            smoothing: 0.22,
            radius: 960,
        }
    }
}

impl MouseFollowConfig {
    /// Apply radius → width/height (16:9), keep padding sensible.
    pub fn apply_radius(&mut self) {
        if self.radius == 0 {
            return;
        }
        let w = (self.radius.saturating_mul(2)).max(320);
        let h = ((w as f32 * 9.0 / 16.0).round() as u32).max(180);
        self.width = w;
        self.height = h;
        self.edge_padding = (self.radius / 8).max(40).min(self.radius / 2);
    }

    pub fn resolved_size(&self) -> (u32, u32) {
        if self.radius > 0 {
            let w = (self.radius.saturating_mul(2)).max(320);
            let h = ((w as f32 * 9.0 / 16.0).round() as u32).max(180);
            (w, h)
        } else {
            (self.width.max(1), self.height.max(1))
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: u32,
    pub height: u32,
}

/// Smoothly track a viewport around the cursor on the virtual canvas.
pub fn follow_viewport(
    cursor_canvas: (f32, f32),
    canvas_w: u32,
    canvas_h: u32,
    cfg: &MouseFollowConfig,
    prev: Option<(f32, f32)>,
) -> Viewport {
    let (rw, rh) = cfg.resolved_size();
    let vw = rw.min(canvas_w).max(1);
    let vh = rh.min(canvas_h).max(1);
    let pad = cfg.edge_padding.min(vw / 3).min(vh / 3) as f32;

    let ideal_x = cursor_canvas.0 - vw as f32 / 2.0;
    let ideal_y = cursor_canvas.1 - vh as f32 / 2.0;

    let mut target_x = ideal_x;
    let mut target_y = ideal_y;

    let local_x = cursor_canvas.0 - ideal_x;
    let local_y = cursor_canvas.1 - ideal_y;
    if local_x < pad {
        target_x = cursor_canvas.0 - pad;
    } else if local_x > vw as f32 - pad {
        target_x = cursor_canvas.0 - (vw as f32 - pad);
    }
    if local_y < pad {
        target_y = cursor_canvas.1 - pad;
    } else if local_y > vh as f32 - pad {
        target_y = cursor_canvas.1 - (vh as f32 - pad);
    }

    let max_x = (canvas_w.saturating_sub(vw)) as f32;
    let max_y = (canvas_h.saturating_sub(vh)) as f32;
    target_x = target_x.clamp(0.0, max_x);
    target_y = target_y.clamp(0.0, max_y);

    let (px, py) = prev.unwrap_or((target_x, target_y));
    let t = cfg.smoothing.clamp(0.01, 1.0);
    let x = px + (target_x - px) * t;
    let y = py + (target_y - py) * t;

    Viewport {
        x,
        y,
        width: vw,
        height: vh,
    }
}

/// Map a global desktop cursor (screen coords) onto canvas coords given monitor placements.
pub fn desktop_cursor_to_canvas(
    desktop_x: i32,
    desktop_y: i32,
    placements: &[crate::layout::MonitorPlacement],
    monitors: &[kynx_capture::MonitorInfo],
) -> Option<(f32, f32)> {
    for p in placements.iter().filter(|p| p.enabled) {
        let Some(m) = monitors.iter().find(|m| m.id == p.monitor_id) else {
            continue;
        };
        let right = m.x + m.width as i32;
        let bottom = m.y + m.height as i32;
        if desktop_x >= m.x && desktop_x < right && desktop_y >= m.y && desktop_y < bottom {
            let local_x = (desktop_x - m.x) as f32 * p.scale;
            let local_y = (desktop_y - m.y) as f32 * p.scale;
            return Some((p.x as f32 + local_x, p.y as f32 + local_y));
        }
    }
    None
}
