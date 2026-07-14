use crate::layout::{LayoutConfig, OutputMode};
use crate::mouse_follow::{desktop_cursor_to_canvas, follow_viewport, Viewport};
use kynx_capture::{CapturedFrame, MonitorInfo};
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ComposedFrame {
    pub width: u32,
    pub height: u32,
    /// BGRA8 tightly packed — Arc so present/preview can share without copies.
    pub pixels: Arc<[u8]>,
    pub viewport: Option<Viewport>,
}

#[derive(Debug, Clone)]
pub struct ComposeResult {
    /// Full canvas — for layout editor preview (None when skipped for perf)
    pub layout: Option<ComposedFrame>,
    /// What Discord/stream sees (cropped when mouse-follow)
    pub output: ComposedFrame,
}

#[derive(Debug, Clone, Copy)]
pub struct ComposeOptions {
    /// Build full native layout canvas (expensive on multi-monitor).
    pub include_layout: bool,
}

impl Default for ComposeOptions {
    fn default() -> Self {
        Self {
            include_layout: true,
        }
    }
}

/// Compose monitor frames. Hot path uses viewport/output-sized buffers only.
pub fn compose_frame(
    frames: &HashMap<String, CapturedFrame>,
    monitors: &[MonitorInfo],
    layout: &LayoutConfig,
    cursor_desktop: Option<(i32, i32)>,
    prev_viewport: Option<(f32, f32)>,
) -> ComposeResult {
    compose_frame_with_options(
        frames,
        monitors,
        layout,
        cursor_desktop,
        prev_viewport,
        ComposeOptions::default(),
    )
}

pub fn compose_frame_with_options(
    frames: &HashMap<String, CapturedFrame>,
    monitors: &[MonitorInfo],
    layout: &LayoutConfig,
    cursor_desktop: Option<(i32, i32)>,
    prev_viewport: Option<(f32, f32)>,
    opts: ComposeOptions,
) -> ComposeResult {
    let (layout_w, layout_h) = crate::layout::compute_native_canvas_size(&layout.placements, monitors);
    let (canvas_w, canvas_h) = layout.resolve_canvas_size(monitors);

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

        let output = compose_into_viewport(frames, monitors, layout, &vp);
        let layout_frame = if opts.include_layout {
            Some(compose_full_layout(frames, monitors, layout, layout_w, layout_h))
        } else {
            None
        };
        return ComposeResult {
            layout: layout_frame,
            output,
        };
    }

    // Static: compose directly at output resolution (no huge intermediate + downscale).
    let output = compose_static_output(frames, monitors, layout, canvas_w, canvas_h, layout_w, layout_h);
    let layout_frame = if opts.include_layout {
        if layout_w == canvas_w && layout_h == canvas_h {
            Some(output.clone())
        } else {
            Some(compose_full_layout(frames, monitors, layout, layout_w, layout_h))
        }
    } else {
        None
    };

    ComposeResult {
        layout: layout_frame,
        output,
    }
}

fn compose_full_layout(
    frames: &HashMap<String, CapturedFrame>,
    monitors: &[MonitorInfo],
    layout: &LayoutConfig,
    layout_w: u32,
    layout_h: u32,
) -> ComposedFrame {
    let mut canvas = vec![0u8; (layout_w as usize) * (layout_h as usize) * 4];
    fill_bgra(&mut canvas, layout.background_bgra);
    blit_all_monitors(
        &mut canvas,
        layout_w,
        layout_h,
        frames,
        monitors,
        layout,
        1.0,
        1.0,
        0,
        0,
    );
    ComposedFrame {
        width: layout_w,
        height: layout_h,
        pixels: canvas.into(),
        viewport: None,
    }
}

fn compose_static_output(
    frames: &HashMap<String, CapturedFrame>,
    monitors: &[MonitorInfo],
    layout: &LayoutConfig,
    out_w: u32,
    out_h: u32,
    native_w: u32,
    native_h: u32,
) -> ComposedFrame {
    let mut canvas = vec![0u8; (out_w as usize) * (out_h as usize) * 4];
    fill_bgra(&mut canvas, layout.background_bgra);
    let sx = if native_w > 0 {
        out_w as f32 / native_w as f32
    } else {
        1.0
    };
    let sy = if native_h > 0 {
        out_h as f32 / native_h as f32
    } else {
        1.0
    };
    blit_all_monitors(
        &mut canvas,
        out_w,
        out_h,
        frames,
        monitors,
        layout,
        sx,
        sy,
        0,
        0,
    );
    ComposedFrame {
        width: out_w,
        height: out_h,
        pixels: canvas.into(),
        viewport: None,
    }
}

/// Blit only the visible viewport region — never allocate the full multi-monitor canvas.
fn compose_into_viewport(
    frames: &HashMap<String, CapturedFrame>,
    monitors: &[MonitorInfo],
    layout: &LayoutConfig,
    vp: &Viewport,
) -> ComposedFrame {
    let mut out = vec![0u8; (vp.width as usize) * (vp.height as usize) * 4];
    fill_bgra(&mut out, layout.background_bgra);

    let vx = vp.x.round() as i32;
    let vy = vp.y.round() as i32;

    for p in layout.placements.iter().filter(|p| p.enabled) {
        let Some(frame) = frames.get(&p.monitor_id) else {
            continue;
        };
        let Some(mon) = monitors.iter().find(|m| m.id == p.monitor_id) else {
            continue;
        };
        let dest_w = ((mon.width as f32) * p.scale).round().max(1.0) as u32;
        let dest_h = ((mon.height as f32) * p.scale).round().max(1.0) as u32;

        // Placement rect in canvas space, shifted so viewport origin is (0,0)
        blit_scaled(
            &mut out,
            vp.width,
            vp.height,
            p.x - vx,
            p.y - vy,
            dest_w,
            dest_h,
            &frame.pixels,
            frame.width,
            frame.height,
            frame.stride,
        );
    }

    ComposedFrame {
        width: vp.width,
        height: vp.height,
        pixels: out.into(),
        viewport: Some(*vp),
    }
}

fn blit_all_monitors(
    canvas: &mut [u8],
    canvas_w: u32,
    canvas_h: u32,
    frames: &HashMap<String, CapturedFrame>,
    monitors: &[MonitorInfo],
    layout: &LayoutConfig,
    scale_x: f32,
    scale_y: f32,
    origin_x: i32,
    origin_y: i32,
) {
    for p in layout.placements.iter().filter(|p| p.enabled) {
        let Some(frame) = frames.get(&p.monitor_id) else {
            continue;
        };
        let Some(mon) = monitors.iter().find(|m| m.id == p.monitor_id) else {
            continue;
        };
        let dest_w = ((mon.width as f32) * p.scale * scale_x).round().max(1.0) as u32;
        let dest_h = ((mon.height as f32) * p.scale * scale_y).round().max(1.0) as u32;
        let dx = ((p.x as f32) * scale_x).round() as i32 - origin_x;
        let dy = ((p.y as f32) * scale_y).round() as i32 - origin_y;
        blit_scaled(
            canvas,
            canvas_w,
            canvas_h,
            dx,
            dy,
            dest_w,
            dest_h,
            &frame.pixels,
            frame.width,
            frame.height,
            frame.stride,
        );
    }
}

fn fill_bgra(buf: &mut [u8], color: [u8; 4]) {
    if buf.is_empty() {
        return;
    }
    // Fast path: solid black / near-black common background
    if color == [0, 0, 0, 0] || color == [0, 0, 0, 255] {
        if color[3] == 0 {
            buf.fill(0);
        } else {
            for chunk in buf.chunks_exact_mut(4) {
                chunk.copy_from_slice(&color);
            }
        }
        return;
    }
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
    if dw == 0 || dh == 0 || sw == 0 || sh == 0 || dst_w == 0 || dst_h == 0 {
        return;
    }

    let x0 = dx.max(0);
    let y0 = dy.max(0);
    let x1 = (dx + dw as i32).min(dst_w as i32);
    let y1 = (dy + dh as i32).min(dst_h as i32);
    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let copy_w = (x1 - x0) as u32;
    let dst_stride = (dst_w * 4) as usize;

    // 1:1 pixel copy — row memcpy
    if sw == dw && sh == dh && stride >= sw * 4 {
        for y in y0..y1 {
            let src_y = (y - dy) as u32;
            if src_y >= sh {
                continue;
            }
            let src_x = (x0 - dx) as u32;
            if src_x >= sw {
                continue;
            }
            let max_copy = (sw - src_x).min(copy_w);
            let si = (src_y * stride + src_x * 4) as usize;
            let di = (y as usize) * dst_stride + (x0 as usize) * 4;
            let bytes = (max_copy as usize) * 4;
            if si + bytes <= src.len() && di + bytes <= dst.len() {
                dst[di..di + bytes].copy_from_slice(&src[si..si + bytes]);
            }
        }
        return;
    }

    // Nearest-neighbor scale — parallelize by destination rows
    let dst_ptr = dst.as_mut_ptr() as usize;
    let dst_len = dst.len();
    let src_ptr = src.as_ptr() as usize;
    let src_len = src.len();

    (y0..y1).into_par_iter().for_each(|y| {
        let src_y = (((y - dy) as u64 * sh as u64) / dh as u64) as u32;
        if src_y >= sh {
            return;
        }
        let row_base = (y as usize) * dst_stride + (x0 as usize) * 4;
        // SAFETY: each rayon task owns a distinct destination row; rows don't overlap.
        let dst_row = unsafe {
            let base = dst_ptr as *mut u8;
            if row_base + (copy_w as usize) * 4 > dst_len {
                return;
            }
            std::slice::from_raw_parts_mut(base.add(row_base), (copy_w as usize) * 4)
        };
        let src = unsafe { std::slice::from_raw_parts(src_ptr as *const u8, src_len) };

        for x_off in 0..copy_w {
            let x = x0 + x_off as i32;
            let src_x = (((x - dx) as u64 * sw as u64) / dw as u64) as u32;
            if src_x >= sw {
                continue;
            }
            let si = (src_y * stride + src_x * 4) as usize;
            let di = (x_off as usize) * 4;
            if si + 4 <= src.len() {
                dst_row[di..di + 4].copy_from_slice(&src[si..si + 4]);
            }
        }
    });
}
