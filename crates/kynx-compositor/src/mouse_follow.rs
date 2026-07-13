use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseFollowConfig {
    /// Viewport width in output pixels
    pub width: u32,
    /// Viewport height in output pixels
    pub height: u32,
    /// Soft edge padding — cursor stays this many px from viewport edge when possible
    pub edge_padding: u32,
    /// Lerp factor per frame (0..1). Higher = snappier.
    pub smoothing: f32,
}

impl Default for MouseFollowConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            edge_padding: 120,
            smoothing: 0.18,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: u32,
    pub height: u32,
}

/// Smoothly track a viewport around the cursor on the virtual canvas.
///
/// `cursor_canvas` is cursor position in canvas coordinates.
/// `prev` is the previous viewport top-left (for smoothing).
pub fn follow_viewport(
    cursor_canvas: (f32, f32),
    canvas_w: u32,
    canvas_h: u32,
    cfg: &MouseFollowConfig,
    prev: Option<(f32, f32)>,
) -> Viewport {
    let vw = cfg.width.min(canvas_w).max(1);
    let vh = cfg.height.min(canvas_h).max(1);
    let pad = cfg.edge_padding as f32;

    // Ideal top-left so cursor sits in center, then clamp so we stay on canvas
    let ideal_x = cursor_canvas.0 - vw as f32 / 2.0;
    let ideal_y = cursor_canvas.1 - vh as f32 / 2.0;

    // Soft bias: if cursor near edge of ideal viewport, prefer keeping padding
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
        let m = monitors.iter().find(|m| m.id == p.monitor_id)?;
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
