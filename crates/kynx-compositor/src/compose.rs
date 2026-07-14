use crate::layout::{LayoutConfig, OutputMode};
use crate::mouse_follow::{desktop_cursor_to_canvas, follow_viewport, Viewport};
use kynx_capture::{CapturedFrame, MonitorInfo};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ComposedFrame {
    pub width: u32,
    pub height: u32,
    /// BGRA8 tightly packed
    pub pixels: Vec<u8>,
    pub viewport: Option<Viewport>,
}

#[derive(Debug, Clone)]
pub struct ComposeResult {
    /// Full canvas — for layout editor preview
    pub layout: ComposedFrame,
    /// What Discord/stream sees (cropped when mouse-follow)
    pub output: ComposedFrame,
}

/// Compose monitor frames onto a canvas; always keep full layout + stream output.
pub fn compose_frame(
    frames: &HashMap<String, CapturedFrame>,
    monitors: &[MonitorInfo],
    layout: &LayoutConfig,
    cursor_desktop: Option<(i32, i32)>,
    prev_viewport: Option<(f32, f32)>,
) -> ComposeResult {
    let (layout_w, layout_h) = crate::layout::compute_native_canvas_size(&layout.placements, monitors);
    let (canvas_w, canvas_h) = layout.resolve_canvas_size(monitors);

    let mut canvas = vec![0u8; (layout_w * layout_h * 4) as usize];
    fill_bgra(&mut canvas, layout.background_bgra);

    for p in layout.placements.iter().filter(|p| p.enabled) {
        let Some(frame) = frames.get(&p.monitor_id) else {
            continue;
        };
        let Some(mon) = monitors.iter().find(|m| m.id == p.monitor_id) else {
            continue;
        };
        let dest_w = ((mon.width as f32) * p.scale).round().max(1.0) as u32;
        let dest_h = ((mon.height as f32) * p.scale).round().max(1.0) as u32;
        blit_scaled(
            &mut canvas,
            layout_w,
            layout_h,
            p.x,
            p.y,
            dest_w,
            dest_h,
            &frame.pixels,
            frame.width,
            frame.height,
            frame.stride,
        );
    }

    let layout_frame = ComposedFrame {
        width: layout_w,
        height: layout_h,
        pixels: canvas.clone(),
        viewport: None,
    };

    if layout.mode == OutputMode::MouseFollow {
        let cursor_canvas = cursor_desktop
            .and_then(|(x, y)| desktop_cursor_to_canvas(x, y, &layout.placements, monitors))
            .unwrap_or((layout_w as f32 / 2.0, layout_h as f32 / 2.0));

        let vp = follow_viewport(
            cursor_canvas,
            layout_w,
            layout_h,
            &layout.follow,
            prev_viewport,
        );
        let cropped = crop_bgra(&canvas, layout_w, layout_h, &vp);
        let output = ComposedFrame {
            width: vp.width,
            height: vp.height,
            pixels: cropped,
            viewport: Some(vp),
        };
        return ComposeResult {
            layout: layout_frame,
            output,
        };
    }

    // Static: optionally downscale to max canvas for output
    let output = if layout_w != canvas_w || layout_h != canvas_h {
        let mut scaled = vec![0u8; (canvas_w * canvas_h * 4) as usize];
        blit_scaled(
            &mut scaled,
            canvas_w,
            canvas_h,
            0,
            0,
            canvas_w,
            canvas_h,
            &canvas,
            layout_w,
            layout_h,
            layout_w * 4,
        );
        ComposedFrame {
            width: canvas_w,
            height: canvas_h,
            pixels: scaled,
            viewport: None,
        }
    } else {
        layout_frame.clone()
    };

    ComposeResult {
        layout: layout_frame,
        output,
    }
}

fn fill_bgra(buf: &mut [u8], color: [u8; 4]) {
    for chunk in buf.chunks_exact_mut(4) {
        chunk.copy_from_slice(&color);
    }
}

fn blit_scaled(
    dst: &mut [u8],
    dst_w: u32,
    dst_h: u32,
    dx: i32,
    dy: i32,
    dw: u32,
    dh: u32,
    src: &[u8],
    sw: u32,
    sh: u32,
    stride: u32,
) {
    if dw == 0 || dh == 0 || sw == 0 || sh == 0 {
        return;
    }
    for y in 0..dh {
        let out_y = dy + y as i32;
        if out_y < 0 || out_y >= dst_h as i32 {
            continue;
        }
        let src_y = ((y as u64 * sh as u64) / dh as u64) as u32;
        for x in 0..dw {
            let out_x = dx + x as i32;
            if out_x < 0 || out_x >= dst_w as i32 {
                continue;
            }
            let src_x = ((x as u64 * sw as u64) / dw as u64) as u32;
            let si = (src_y * stride + src_x * 4) as usize;
            let di = ((out_y as u32 * dst_w + out_x as u32) * 4) as usize;
            if si + 4 <= src.len() && di + 4 <= dst.len() {
                dst[di..di + 4].copy_from_slice(&src[si..si + 4]);
            }
        }
    }
}

fn crop_bgra(src: &[u8], src_w: u32, src_h: u32, vp: &Viewport) -> Vec<u8> {
    let x0 = vp.x.round().max(0.0) as u32;
    let y0 = vp.y.round().max(0.0) as u32;
    let mut out = vec![0u8; (vp.width * vp.height * 4) as usize];
    for y in 0..vp.height {
        let sy = y0 + y;
        if sy >= src_h {
            continue;
        }
        for x in 0..vp.width {
            let sx = x0 + x;
            if sx >= src_w {
                continue;
            }
            let si = ((sy * src_w + sx) * 4) as usize;
            let di = ((y * vp.width + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    out
}
