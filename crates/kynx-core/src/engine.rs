use crate::config::AppConfig;
use anyhow::{Context, Result};
use kynx_capture::{enumerate_monitors, MonitorInfo, MultiCapture};
use kynx_compositor::{
    compose_frame_with_options, sync_layout_with_monitors, ComposeOptions, LayoutConfig, OutputMode,
};
use kynx_output::{
    detect_virtual_display_driver, is_virtual_monitor, open_driver_installer, ShareWindow,
    VirtualCamera, VirtualDisplaySession, VirtualDisplayStatus,
};
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

/// UI preview encode cadence (~12 FPS). Keeps the compose/present path at target FPS.
const PREVIEW_INTERVAL: Duration = Duration::from_millis(80);
const PREVIEW_MAX_WIDTH: u32 = 960;

#[derive(Debug, Clone, Serialize)]
pub struct EngineStatus {
    pub running: bool,
    pub output_active: bool,
    pub mode: OutputMode,
    pub fps: f32,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub monitor_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineSnapshot {
    pub status: EngineStatus,
    pub monitors: Vec<MonitorInfo>,
    pub layout: LayoutConfig,
    pub vdd: VirtualDisplayStatus,
    pub layout_preview_jpeg_base64: Option<String>,
    pub output_preview_jpeg_base64: Option<String>,
    /// @deprecated alias for layout preview
    pub preview_jpeg_base64: Option<String>,
}

pub struct KynxEngine {
    config: Arc<Mutex<AppConfig>>,
    monitors: Arc<Mutex<Vec<MonitorInfo>>>,
    capture: Arc<Mutex<Option<MultiCapture>>>,
    share: Arc<Mutex<Option<ShareWindow>>>,
    vcam: Arc<Mutex<Option<VirtualCamera>>>,
    vdd_session: Arc<Mutex<Option<VirtualDisplaySession>>>,
    running: Arc<AtomicBool>,
    output_active: Arc<AtomicBool>,
    loop_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    latest_layout_preview: Arc<Mutex<Option<Vec<u8>>>>,
    latest_output_preview: Arc<Mutex<Option<Vec<u8>>>>,
    latest_status: Arc<Mutex<EngineStatus>>,
    prev_viewport: Arc<Mutex<Option<(f32, f32)>>>,
}

impl KynxEngine {
    pub fn new(config: AppConfig) -> Result<Self> {
        let monitors = enumerate_monitors().unwrap_or_default();
        let mut cfg = config;
        if cfg.layout.placements.is_empty() && !monitors.is_empty() {
            cfg.layout = LayoutConfig::from_monitors(&monitors);
            cfg.selected_monitor_ids = monitors.iter().map(|m| m.id.clone()).collect();
        } else if !monitors.is_empty() {
            sync_layout_with_monitors(&mut cfg.layout, &monitors);
            cfg.selected_monitor_ids = cfg
                .layout
                .placements
                .iter()
                .filter(|p| p.enabled)
                .map(|p| p.monitor_id.clone())
                .collect();
        }
        cfg.layout.follow.apply_radius();
        // Migrate legacy defaults; never keep VDD auto-plug on.
        if cfg.target_fps == 30 {
            cfg.target_fps = 60;
        }
        cfg.outputs.virtual_display = false;
        let (cw, ch) = cfg.layout.native_canvas_size(&monitors);
        let (ow, oh) = cfg.layout.resolve_canvas_size(&monitors);
        Ok(Self {
            config: Arc::new(Mutex::new(cfg.clone())),
            monitors: Arc::new(Mutex::new(monitors.clone())),
            capture: Arc::new(Mutex::new(None)),
            share: Arc::new(Mutex::new(None)),
            vcam: Arc::new(Mutex::new(None)),
            vdd_session: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            output_active: Arc::new(AtomicBool::new(false)),
            loop_handle: Mutex::new(None),
            latest_layout_preview: Arc::new(Mutex::new(None)),
            latest_output_preview: Arc::new(Mutex::new(None)),
            latest_status: Arc::new(Mutex::new(EngineStatus {
                running: false,
                output_active: false,
                mode: cfg.layout.mode,
                fps: 0.0,
                canvas_width: cw,
                canvas_height: ch,
                output_width: ow,
                output_height: oh,
                monitor_count: monitors.len(),
            })),
            prev_viewport: Arc::new(Mutex::new(None)),
        })
    }

    pub fn refresh_monitors(&self) -> Result<Vec<MonitorInfo>> {
        let monitors = enumerate_monitors()?;
        *self.monitors.lock() = monitors.clone();
        let mut cfg = self.config.lock().clone();
        if cfg.layout.placements.is_empty() {
            cfg.layout = LayoutConfig::from_monitors(&monitors);
        } else {
            sync_layout_with_monitors(&mut cfg.layout, &monitors);
        }
        for p in &mut cfg.layout.placements {
            if let Some(m) = monitors.iter().find(|m| m.id == p.monitor_id) {
                if is_virtual_monitor(&m.name, &m.device_name) {
                    p.enabled = false;
                }
            }
        }
        cfg.selected_monitor_ids = cfg
            .layout
            .placements
            .iter()
            .filter(|p| p.enabled)
            .map(|p| p.monitor_id.clone())
            .collect();
        *self.config.lock() = cfg.clone();
        self.refresh_status_dims(&cfg, &monitors);
        Ok(monitors)
    }

    pub fn apply_desktop_layout(&self) -> Result<()> {
        let monitors = self.monitors.lock().clone();
        let mut cfg = self.config.lock().clone();
        kynx_compositor::apply_desktop_arrangement(&mut cfg.layout, &monitors);
        cfg.selected_monitor_ids = monitors.iter().map(|m| m.id.clone()).collect();
        *self.config.lock() = cfg.clone();
        self.refresh_status_dims(&cfg, &monitors);
        Ok(())
    }

    pub fn get_config(&self) -> AppConfig {
        self.config.lock().clone()
    }

    /// Apply config in memory (live preview) without writing disk.
    pub fn apply_config(&self, mut cfg: AppConfig) -> Result<()> {
        cfg.layout.follow.apply_radius();
        // Hard deny: never allow enabling VDD auto-plug from UI/config.
        cfg.outputs.virtual_display = false;
        let monitors = self.monitors.lock().clone();
        let (w, h) = cfg.layout.native_canvas_size(&monitors);
        cfg.layout.canvas_width = Some(w);
        cfg.layout.canvas_height = Some(h);
        if let Some(share) = self.share.lock().as_ref() {
            share.set_visible(cfg.outputs.show_share_window);
        }
        if let Some(vcam) = self.vcam.lock().as_ref() {
            vcam.set_enabled(
                cfg.outputs.virtual_camera && self.output_active.load(Ordering::SeqCst),
            );
        }
        *self.config.lock() = cfg.clone();
        self.refresh_status_dims(&cfg, &monitors);
        Ok(())
    }

    /// Persist current in-memory config to disk.
    pub fn persist_config(&self) -> Result<()> {
        self.config.lock().save()
    }

    /// Apply + persist (legacy).
    pub fn update_config(&self, cfg: AppConfig) -> Result<()> {
        self.apply_config(cfg)?;
        self.persist_config()
    }

    fn refresh_status_dims(&self, cfg: &AppConfig, monitors: &[MonitorInfo]) {
        let (cw, ch) = cfg.layout.native_canvas_size(monitors);
        let (ow, oh) = cfg.layout.resolve_canvas_size(monitors);
        let mut st = self.latest_status.lock();
        st.canvas_width = cw;
        st.canvas_height = ch;
        // For mouse-follow, output size is follow viewport; approximate until next frame
        if cfg.layout.mode == OutputMode::MouseFollow {
            let (fw, fh) = cfg.layout.follow.resolved_size();
            st.output_width = fw.min(cw).max(1);
            st.output_height = fh.min(ch).max(1);
        } else {
            st.output_width = ow;
            st.output_height = oh;
        }
        st.mode = cfg.layout.mode;
        st.monitor_count = monitors.len();
    }

    pub fn status(&self) -> EngineStatus {
        self.latest_status.lock().clone()
    }

    pub fn layout_preview_jpeg(&self) -> Option<Vec<u8>> {
        self.latest_layout_preview.lock().clone()
    }

    pub fn output_preview_jpeg(&self) -> Option<Vec<u8>> {
        self.latest_output_preview.lock().clone()
    }

    pub fn preview_jpeg(&self) -> Option<Vec<u8>> {
        self.layout_preview_jpeg()
    }

    pub fn vdd_status(&self) -> VirtualDisplayStatus {
        let mut st = detect_virtual_display_driver();
        if let Some(sess) = self.vdd_session.lock().as_ref() {
            st.active_index = Some(sess.index);
            st.monitor_device = sess.monitor_device.clone();
            st.installed = true;
            st.driver_ok = true;
        }
        st
    }

    pub fn open_vdd_installer(&self) -> Result<()> {
        open_driver_installer()
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        let cfg = self.config.lock().clone();
        let monitors = self.monitors.lock().clone();
        let layout_p = self.layout_preview_jpeg().map(|b| base64_encode(&b));
        let output_p = self.output_preview_jpeg().map(|b| base64_encode(&b));
        EngineSnapshot {
            status: self.status(),
            monitors,
            layout: cfg.layout,
            vdd: self.vdd_status(),
            layout_preview_jpeg_base64: layout_p.clone(),
            output_preview_jpeg_base64: output_p,
            preview_jpeg_base64: layout_p,
        }
    }

    /// Start capture + compose for live previews (no stream output required).
    pub fn ensure_preview(&self) -> Result<()> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }
        let _ = self.refresh_monitors();
        let monitors = self.monitors.lock().clone();
        let cfg = self.config.lock().clone();
        let selected: Vec<MonitorInfo> = monitors
            .iter()
            .filter(|m| !is_virtual_monitor(&m.name, &m.device_name))
            .filter(|m| {
                cfg.layout
                    .placements
                    .iter()
                    .any(|p| p.monitor_id == m.id && p.enabled)
            })
            .cloned()
            .collect();

        if selected.is_empty() {
            anyhow::bail!("no monitors enabled");
        }

        let capture = MultiCapture::start(selected).context("start capture")?;
        *self.capture.lock() = Some(capture);

        // Virtual cam mapping ready; disabled until streaming
        let mut vcam_cfg = cfg.virtual_camera.clone();
        vcam_cfg.enabled = false;
        match VirtualCamera::open(vcam_cfg) {
            Ok(v) => *self.vcam.lock() = Some(v),
            Err(e) => warn!("virtual camera mapping failed: {e}"),
        }

        self.running.store(true, Ordering::SeqCst);
        self.output_active.store(false, Ordering::SeqCst);
        self.spawn_loop();
        info!("kynxShare preview running");
        Ok(())
    }

    /// Start streaming via share window only.
    /// Virtual-monitor auto-plug is intentionally disabled (system stability).
    pub fn start(&self) -> Result<()> {
        self.ensure_preview()?;
        let mut cfg = self.config.lock().clone();
        cfg.outputs.share_window = true;
        cfg.outputs.virtual_display = false;
        cfg.outputs.ui_live_preview = false;
        cfg.outputs.show_share_window = true;

        // Tear down any previous VDD attempt (should be none).
        if let Some(prev) = self.vdd_session.lock().take() {
            prev.stop();
        }

        if self.share.lock().is_none() {
            let mut sw_cfg = cfg.share_window.clone();
            sw_cfg.title = "kynxShare Output".into();
            sw_cfg.visible = true;
            sw_cfg.always_on_top = cfg.outputs.always_on_top;
            match ShareWindow::create(sw_cfg) {
                Ok(w) => *self.share.lock() = Some(w),
                Err(e) => warn!("share window failed: {e}"),
            }
        }
        if let Some(w) = self.share.lock().as_ref() {
            w.show_for_capture();
        }

        *self.config.lock() = cfg.clone();
        self.output_active.store(true, Ordering::SeqCst);
        if let Some(vcam) = self.vcam.lock().as_ref() {
            vcam.set_enabled(cfg.outputs.virtual_camera);
        }
        info!("kynxShare streaming active — Discord: Fenster → kynxShare Output");
        Ok(())
    }

    fn spawn_loop(&self) {
        if self.loop_handle.lock().is_some() {
            return;
        }

        let config = Arc::clone(&self.config);
        let monitors_arc = Arc::clone(&self.monitors);
        let capture = Arc::clone(&self.capture);
        let share = Arc::clone(&self.share);
        let vcam = Arc::clone(&self.vcam);
        let running = Arc::clone(&self.running);
        let output_active = Arc::clone(&self.output_active);
        let latest_layout_preview = Arc::clone(&self.latest_layout_preview);
        let latest_output_preview = Arc::clone(&self.latest_output_preview);
        let latest_status = Arc::clone(&self.latest_status);
        let prev_viewport = Arc::clone(&self.prev_viewport);

        let handle = std::thread::Builder::new()
            .name("kynx-engine".into())
            .spawn(move || {
                let mut last_fps_t = Instant::now();
                let mut frames = 0u32;
                let mut fps = 0.0f32;
                let mut last_preview = Instant::now()
                    .checked_sub(PREVIEW_INTERVAL)
                    .unwrap_or_else(Instant::now);
                let mut canvas_w = 0u32;
                let mut canvas_h = 0u32;

                while running.load(Ordering::SeqCst) {
                    let cfg = config.lock().clone();
                    let target_fps = cfg.target_fps.max(1).min(240);
                    let frame_budget = Duration::from_secs_f64(1.0 / target_fps as f64);
                    let start = Instant::now();

                    let monitors = monitors_arc.lock().clone();
                    let frames_map = {
                        let guard = capture.lock();
                        guard.as_ref().map(|c| c.snapshot()).unwrap_or_default()
                    };

                    let need_layout_preview = cfg.outputs.ui_live_preview
                        && last_preview.elapsed() >= PREVIEW_INTERVAL;
                    let cursor = get_cursor_pos();
                    let prev = *prev_viewport.lock();
                    let composed = compose_frame_with_options(
                        &frames_map,
                        &monitors,
                        &cfg.layout,
                        cursor,
                        prev,
                        ComposeOptions {
                            include_layout: need_layout_preview,
                        },
                    );
                    if let Some(vp) = composed.output.viewport {
                        *prev_viewport.lock() = Some((vp.x, vp.y));
                    }

                    if let Some(layout) = &composed.layout {
                        canvas_w = layout.width;
                        canvas_h = layout.height;
                    } else if canvas_w == 0 {
                        let (cw, ch) = cfg.layout.native_canvas_size(&monitors);
                        canvas_w = cw;
                        canvas_h = ch;
                    }

                    if output_active.load(Ordering::SeqCst) {
                        if let Some(w) = share.lock().as_ref() {
                            let _ = w.present_arc(
                                composed.output.width,
                                composed.output.height,
                                Arc::clone(&composed.output.pixels),
                            );
                        }
                        if let Some(v) = vcam.lock().as_ref() {
                            let ts = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64;
                            v.push_frame(
                                composed.output.width,
                                composed.output.height,
                                &composed.output.pixels,
                                ts,
                            );
                        }
                    }

                    // JPEG previews are UI-only — never block the 60 FPS present path every frame.
                    if need_layout_preview {
                        if let Some(layout) = &composed.layout {
                            if let Some(jpeg) = encode_preview_jpeg(
                                &layout.pixels,
                                layout.width,
                                layout.height,
                                PREVIEW_MAX_WIDTH,
                            ) {
                                *latest_layout_preview.lock() = Some(jpeg);
                            }
                        }
                        if let Some(jpeg) = encode_preview_jpeg(
                            &composed.output.pixels,
                            composed.output.width,
                            composed.output.height,
                            PREVIEW_MAX_WIDTH,
                        ) {
                            *latest_output_preview.lock() = Some(jpeg);
                        }
                        last_preview = Instant::now();
                    }

                    frames += 1;
                    if last_fps_t.elapsed() >= Duration::from_secs(1) {
                        fps = frames as f32 / last_fps_t.elapsed().as_secs_f32();
                        frames = 0;
                        last_fps_t = Instant::now();
                    }

                    *latest_status.lock() = EngineStatus {
                        running: true,
                        output_active: output_active.load(Ordering::SeqCst),
                        mode: cfg.layout.mode,
                        fps,
                        canvas_width: canvas_w,
                        canvas_height: canvas_h,
                        output_width: composed.output.width,
                        output_height: composed.output.height,
                        monitor_count: monitors.len(),
                    };

                    let elapsed = start.elapsed();
                    if elapsed < frame_budget {
                        std::thread::sleep(frame_budget - elapsed);
                    }
                }
            })
            .expect("spawn engine loop");

        *self.loop_handle.lock() = Some(handle);
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        self.output_active.store(false, Ordering::SeqCst);
        if let Some(sess) = self.vdd_session.lock().take() {
            sess.stop();
        }
        if let Some(h) = self.loop_handle.lock().take() {
            let _ = h.join();
        }
        if let Some(c) = self.capture.lock().take() {
            c.stop();
        }
        if let Some(s) = self.share.lock().take() {
            s.close();
        }
        if let Some(v) = self.vcam.lock().take() {
            v.close();
        }
        let mut st = self.latest_status.lock();
        st.running = false;
        st.output_active = false;
        info!("kynxShare engine stopped");
    }

    pub fn set_output_active(&self, active: bool) {
        self.output_active.store(active, Ordering::SeqCst);
        if active {
            // Ensure Discord-visible share window exists
            let cfg = self.config.lock().clone();
            if self.share.lock().is_none() {
                let mut sw_cfg = cfg.share_window.clone();
                sw_cfg.title = "kynxShare Output".into();
                sw_cfg.visible = true;
                if let Ok(w) = ShareWindow::create(sw_cfg) {
                    *self.share.lock() = Some(w);
                }
            }
            if let Some(share) = self.share.lock().as_ref() {
                share.show_for_capture();
            }
            let mut cfg = self.config.lock().clone();
            cfg.outputs.show_share_window = true;
            *self.config.lock() = cfg;
        }
        if let Some(vcam) = self.vcam.lock().as_ref() {
            let enabled = active && self.config.lock().outputs.virtual_camera;
            vcam.set_enabled(enabled);
        }
    }

    pub fn toggle_mode(&self) -> Result<OutputMode> {
        let mut cfg = self.config.lock().clone();
        cfg.layout.mode = match cfg.layout.mode {
            OutputMode::StaticLayout => OutputMode::MouseFollow,
            OutputMode::MouseFollow => OutputMode::StaticLayout,
        };
        let mode = cfg.layout.mode;
        let monitors = self.monitors.lock().clone();
        *self.config.lock() = cfg.clone();
        self.refresh_status_dims(&cfg, &monitors);
        Ok(mode)
    }
}

impl Drop for KynxEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

fn get_cursor_pos() -> Option<(i32, i32)> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        unsafe {
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_ok() {
                return Some((pt.x, pt.y));
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn encode_preview_jpeg(bgra: &[u8], width: u32, height: u32, max_w: u32) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || bgra.len() < (width * height * 4) as usize {
        return None;
    }
    let scale = (max_w as f32 / width as f32).min(1.0);
    let pw = ((width as f32 * scale).round() as u32).max(1);
    let ph = ((height as f32 * scale).round() as u32).max(1);
    let mut rgb = vec![0u8; (pw * ph * 3) as usize];
    for y in 0..ph {
        let sy = ((y as u64 * height as u64) / ph as u64) as u32;
        for x in 0..pw {
            let sx = ((x as u64 * width as u64) / pw as u64) as u32;
            let si = ((sy * width + sx) * 4) as usize;
            let di = ((y * pw + x) * 3) as usize;
            rgb[di] = bgra[si + 2];
            rgb[di + 1] = bgra[si + 1];
            rgb[di + 2] = bgra[si];
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 65);
    encoder
        .encode(&rgb, pw, ph, image::ExtendedColorType::Rgb8)
        .ok()?;
    Some(out.into_inner())
}

fn base64_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let a = chunk[0] as u32;
        let b = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let c = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (a << 16) | (b << 8) | c;
        out.push(T[((triple >> 18) & 63) as usize] as char);
        out.push(T[((triple >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((triple >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(triple & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
