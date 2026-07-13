//! Layout engine, compositor, and mouse-follow viewport.

mod compose;
mod layout;
mod mouse_follow;

pub use compose::{compose_frame, ComposedFrame};
pub use layout::{
    auto_layout_side_by_side, compute_native_canvas_size, snap_placement, LayoutConfig,
    MonitorPlacement, OutputMode,
};
pub use mouse_follow::{follow_viewport, MouseFollowConfig, Viewport};

#[cfg(test)]
mod tests {
    use super::*;
    use kynx_capture::MonitorInfo;

    fn mon(id: &str, x: i32, y: i32, w: u32, h: u32) -> MonitorInfo {
        MonitorInfo {
            id: id.into(),
            name: id.into(),
            device_name: id.into(),
            adapter_index: 0,
            output_index: 0,
            x,
            y,
            width: w,
            height: h,
            refresh_hz: 60,
            is_primary: id == "a",
            scale_percent: 100,
        }
    }

    #[test]
    fn side_by_side_canvas() {
        let monitors = vec![mon("a", 0, 0, 1920, 1080), mon("b", 1920, 0, 2560, 1440)];
        let placements = auto_layout_side_by_side(&monitors);
        assert_eq!(placements.len(), 2);
        assert_eq!(placements[0].x, 0);
        assert_eq!(placements[1].x, 1920);
        let (cw, ch) = compute_native_canvas_size(&placements, &monitors);
        assert_eq!(cw, 1920 + 2560);
        assert_eq!(ch, 1440);
    }

    #[test]
    fn mouse_follow_clamps() {
        let cfg = MouseFollowConfig {
            width: 1920,
            height: 1080,
            edge_padding: 100,
            smoothing: 1.0,
        };
        let vp = follow_viewport((50.0, 50.0), 3840, 1080, &cfg, None);
        assert_eq!(vp.width, 1920);
        assert_eq!(vp.height, 1080);
        assert!(vp.x >= 0.0);
        assert!(vp.y >= 0.0);
    }
}
