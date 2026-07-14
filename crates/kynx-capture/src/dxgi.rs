//! DXGI Desktop Duplication capture implementation.

use crate::monitor::MonitorInfo;
use crate::types::{CaptureError, CapturedFrame, FramePixelFormat};
use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};
use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ,
    D3D11_SDK_VERSION, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_MODE_ROTATION, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput, IDXGIOutput1,
    IDXGIOutputDuplication, IDXGIResource, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT,
    DXGI_OUTDUPL_FRAME_INFO,
};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFOEXW};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

#[derive(Clone)]
struct DxgiMatch {
    adapter_index: u32,
    output_index: u32,
    device_name: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn stable_id(device_name: &str, x: i32, y: i32) -> String {
    format!("{device_name}@{x},{y}")
}

pub fn enumerate_monitors_dxgi() -> Result<Vec<MonitorInfo>> {
    unsafe {
        let dxgi = collect_dxgi_outputs();
        let mut monitors = enumerate_via_gdi(&dxgi);

        if monitors.is_empty() {
            for (i, d) in dxgi.iter().enumerate() {
                monitors.push(MonitorInfo {
                    id: stable_id(&d.device_name, d.x, d.y),
                    name: format!("Display {}", i + 1),
                    device_name: d.device_name.clone(),
                    adapter_index: d.adapter_index,
                    output_index: d.output_index,
                    x: d.x,
                    y: d.y,
                    width: d.width,
                    height: d.height,
                    refresh_hz: 60,
                    is_primary: i == 0,
                    scale_percent: 100,
                });
            }
        }

        // Merge DXGI-only outputs GDI may have missed (multi-GPU)
        for d in &dxgi {
            let exists = monitors.iter().any(|m| {
                m.device_name.eq_ignore_ascii_case(&d.device_name)
                    || (m.x == d.x && m.y == d.y && m.width == d.width && m.height == d.height)
            });
            if exists {
                continue;
            }
            let idx = monitors.len() + 1;
            monitors.push(MonitorInfo {
                id: stable_id(&d.device_name, d.x, d.y),
                name: format!("Display {idx}"),
                device_name: d.device_name.clone(),
                adapter_index: d.adapter_index,
                output_index: d.output_index,
                x: d.x,
                y: d.y,
                width: d.width,
                height: d.height,
                refresh_hz: 60,
                is_primary: false,
                scale_percent: 100,
            });
        }

        monitors.sort_by_key(|m| (m.y, m.x));
        for (i, m) in monitors.iter_mut().enumerate() {
            m.name = if m.is_primary {
                format!("Display {} (Primary)", i + 1)
            } else {
                format!("Display {}", i + 1)
            };
        }
        Ok(monitors)
    }
}

unsafe fn collect_dxgi_outputs() -> Vec<DxgiMatch> {
    let mut out = Vec::new();
    let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
        return out;
    };
    let mut adapter_index = 0u32;
    loop {
        let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(adapter_index) {
            Ok(a) => a,
            Err(_) => break,
        };
        let mut output_index = 0u32;
        loop {
            let output: IDXGIOutput = match adapter.EnumOutputs(output_index) {
                Ok(o) => o,
                Err(_) => break,
            };
            if let Ok(desc) = output.GetDesc() {
                if desc.AttachedToDesktop.as_bool() {
                    let device_name = wchar_to_string(&desc.DeviceName);
                    let rect = desc.DesktopCoordinates;
                    out.push(DxgiMatch {
                        adapter_index,
                        output_index,
                        device_name,
                        x: rect.left,
                        y: rect.top,
                        width: (rect.right - rect.left).max(0) as u32,
                        height: (rect.bottom - rect.top).max(0) as u32,
                    });
                }
            }
            output_index += 1;
        }
        adapter_index += 1;
    }
    out
}

struct EnumCtx {
    dxgi: Vec<DxgiMatch>,
    monitors: Vec<MonitorInfo>,
}

unsafe extern "system" fn monitor_enum_proc(
    hmonitor: windows::Win32::Graphics::Gdi::HMONITOR,
    _hdc: windows::Win32::Graphics::Gdi::HDC,
    _lprc: *mut windows::Win32::Foundation::RECT,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::core::BOOL {
    let ctx = &mut *(lparam.0 as *mut EnumCtx);
    let mut mi = MONITORINFOEXW {
        monitorInfo: windows::Win32::Graphics::Gdi::MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    if !GetMonitorInfoW(hmonitor, &mut mi as *mut _ as *mut _).as_bool() {
        return windows::core::BOOL(1);
    }

    let device_name = wchar_to_string(&mi.szDevice);
    let rc = mi.monitorInfo.rcMonitor;
    let x = rc.left;
    let y = rc.top;
    let width = (rc.right - rc.left).max(0) as u32;
    let height = (rc.bottom - rc.top).max(0) as u32;
    let is_primary = (mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY) != 0;

    let matched = ctx
        .dxgi
        .iter()
        .find(|d| d.device_name.eq_ignore_ascii_case(&device_name))
        .or_else(|| {
            ctx.dxgi
                .iter()
                .find(|d| d.x == x && d.y == y && d.width == width && d.height == height)
        });

    let (adapter_index, output_index) = matched
        .map(|d| (d.adapter_index, d.output_index))
        .unwrap_or((0, ctx.monitors.len() as u32));

    ctx.monitors.push(MonitorInfo {
        id: stable_id(&device_name, x, y),
        name: String::new(),
        device_name,
        adapter_index,
        output_index,
        x,
        y,
        width,
        height,
        refresh_hz: 60,
        is_primary,
        scale_percent: 100,
    });

    windows::core::BOOL(1)
}

unsafe fn enumerate_via_gdi(dxgi: &[DxgiMatch]) -> Vec<MonitorInfo> {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::Graphics::Gdi::EnumDisplayMonitors;

    let mut ctx = EnumCtx {
        dxgi: dxgi.to_vec(),
        monitors: Vec::new(),
    };
    let _ = EnumDisplayMonitors(
        None,
        None,
        Some(monitor_enum_proc),
        LPARAM(&mut ctx as *mut _ as isize),
    );
    ctx.monitors
}

fn wchar_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

struct DxgiOutputCapture {
    monitor: MonitorInfo,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    staging: Option<ID3D11Texture2D>,
    staging_w: u32,
    staging_h: u32,
}

impl DxgiOutputCapture {
    unsafe fn new(monitor: MonitorInfo) -> Result<Self> {
        let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
        let adapter: IDXGIAdapter1 = factory.EnumAdapters1(monitor.adapter_index)?;
        let output: IDXGIOutput = adapter.EnumOutputs(monitor.output_index)?;
        let output1: IDXGIOutput1 = output.cast()?;

        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        let feature_levels = [D3D_FEATURE_LEVEL_11_0];
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&feature_levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;

        let device = device.ok_or_else(|| anyhow!("failed to create D3D11 device"))?;
        let context = context.ok_or_else(|| anyhow!("failed to create D3D11 context"))?;

        let duplication: IDXGIOutputDuplication = output1.DuplicateOutput(&device)?;

        Ok(Self {
            monitor,
            device,
            context,
            duplication,
            staging: None,
            staging_w: 0,
            staging_h: 0,
        })
    }

    unsafe fn ensure_staging(&mut self, w: u32, h: u32) -> Result<()> {
        if self.staging.is_some() && self.staging_w == w && self.staging_h == h {
            return Ok(());
        }
        let desc = windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut tex: Option<ID3D11Texture2D> = None;
        self.device.CreateTexture2D(&desc, None, Some(&mut tex))?;
        self.staging = tex;
        self.staging_w = w;
        self.staging_h = h;
        Ok(())
    }

    unsafe fn capture_frame(&mut self, timeout_ms: u32) -> Result<CapturedFrame, CaptureError> {
        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;

        match self
            .duplication
            .AcquireNextFrame(timeout_ms, &mut frame_info, &mut resource)
        {
            Ok(()) => {}
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                return Err(CaptureError::Timeout);
            }
            Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                return Err(CaptureError::AccessLost);
            }
            Err(e) => {
                return Err(CaptureError::Graphics(e.to_string()));
            }
        }

        let resource = resource.ok_or_else(|| {
            CaptureError::Graphics("AcquireNextFrame returned no resource".into())
        })?;

        let texture: ID3D11Texture2D = resource
            .cast()
            .map_err(|e| CaptureError::Graphics(e.to_string()))?;

        let mut desc = windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);

        self.ensure_staging(desc.Width, desc.Height)
            .map_err(|e| CaptureError::Graphics(e.to_string()))?;

        let staging = self
            .staging
            .as_ref()
            .ok_or_else(|| CaptureError::Graphics("staging texture missing".into()))?;

        self.context.CopyResource(staging, &texture);

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        self.context
            .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .map_err(|e| CaptureError::Graphics(e.to_string()))?;

        let width = desc.Width;
        let height = desc.Height;
        let row_pitch = mapped.RowPitch as usize;
        let src = std::slice::from_raw_parts(mapped.pData as *const u8, row_pitch * height as usize);

        let stride = width * 4;
        let mut pixels = vec![0u8; (stride * height) as usize];
        for y in 0..height as usize {
            let src_row = &src[y * row_pitch..y * row_pitch + stride as usize];
            let dst_row = &mut pixels[y * stride as usize..(y + 1) * stride as usize];
            dst_row.copy_from_slice(src_row);
        }

        self.context.Unmap(staging, 0);
        let _ = self.duplication.ReleaseFrame();

        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(CapturedFrame {
            monitor_id: self.monitor.id.clone(),
            width,
            height,
            stride,
            format: FramePixelFormat::Bgra8,
            pixels,
            timestamp_ms,
        })
    }

    unsafe fn recreate(&mut self) -> Result<(), CaptureError> {
        match Self::new(self.monitor.clone()) {
            Ok(fresh) => {
                *self = fresh;
                Ok(())
            }
            Err(e) => Err(CaptureError::Graphics(e.to_string())),
        }
    }
}

/// Single-monitor capture session.
pub struct CaptureSession {
    inner: Mutex<DxgiOutputCapture>,
}

impl CaptureSession {
    pub fn open(monitor: MonitorInfo) -> Result<Self> {
        unsafe {
            Ok(Self {
                inner: Mutex::new(DxgiOutputCapture::new(monitor)?),
            })
        }
    }

    pub fn capture(&self, timeout_ms: u32) -> Result<CapturedFrame, CaptureError> {
        unsafe {
            let mut guard = self.inner.lock();
            match guard.capture_frame(timeout_ms) {
                Err(CaptureError::AccessLost) => {
                    warn!("DXGI access lost on {}, recreating", guard.monitor.id);
                    guard.recreate()?;
                    guard.capture_frame(timeout_ms)
                }
                other => other,
            }
        }
    }

    pub fn monitor(&self) -> MonitorInfo {
        self.inner.lock().monitor.clone()
    }
}

/// Captures multiple monitors concurrently into a shared frame map.
pub struct MultiCapture {
    sessions: Vec<Arc<CaptureSession>>,
    running: Arc<AtomicBool>,
    latest: Arc<Mutex<std::collections::HashMap<String, CapturedFrame>>>,
    handles: Mutex<Vec<std::thread::JoinHandle<()>>>,
}

impl MultiCapture {
    pub fn start(monitors: Vec<MonitorInfo>) -> Result<Self> {
        let mut sessions = Vec::new();
        for m in monitors {
            sessions.push(Arc::new(CaptureSession::open(m)?));
        }
        let latest = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let running = Arc::new(AtomicBool::new(true));
        let mut handles = Vec::new();

        for session in &sessions {
            let session = Arc::clone(session);
            let latest = Arc::clone(&latest);
            let running = Arc::clone(&running);
            let handle = std::thread::Builder::new()
                .name(format!("capture-{}", session.monitor().id))
                .spawn(move || {
                    while running.load(Ordering::SeqCst) {
                        match session.capture(16) {
                            Ok(frame) => {
                                latest.lock().insert(frame.monitor_id.clone(), frame);
                            }
                            Err(CaptureError::Timeout) => {}
                            Err(e) => {
                                debug!("capture error: {e}");
                                std::thread::sleep(Duration::from_millis(50));
                            }
                        }
                    }
                })
                .context("spawn capture thread")?;
            handles.push(handle);
        }

        Ok(Self {
            sessions,
            running,
            latest,
            handles: Mutex::new(handles),
        })
    }

    pub fn snapshot(&self) -> std::collections::HashMap<String, CapturedFrame> {
        self.latest.lock().clone()
    }

    pub fn monitors(&self) -> Vec<MonitorInfo> {
        self.sessions.iter().map(|s| s.monitor()).collect()
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        let mut handles = self.handles.lock();
        for h in handles.drain(..) {
            let _ = h.join();
        }
    }
}

impl Drop for MultiCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

// Silence unused import warnings for rotation enum on some SDK versions
#[allow(dead_code)]
fn _rotation_unused(_: DXGI_MODE_ROTATION) {}

#[allow(dead_code)]
fn _hwnd_unused(_: HWND) {}

#[allow(dead_code)]
fn _instant_unused(_: Instant) {}
