use super::ShareWindowConfig;
use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateCompatibleDC, CreateDIBSection, CreateSolidBrush, DeleteDC, DeleteObject,
    EndPaint, FillRect, InvalidateRect, SelectObject, StretchBlt, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, SRCCOPY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
    GetWindowLongPtrW, LoadCursorW, PeekMessageW, PostMessageW, RegisterClassW, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, IDC_ARROW,
    MSG, PM_REMOVE, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SW_HIDE, SW_SHOW, WM_DESTROY, WM_ERASEBKGND, WM_PAINT, WM_QUIT, WM_USER, WNDCLASSW,
    WS_EX_APPWINDOW, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW, WS_POPUP, HWND_TOPMOST, GWL_STYLE,
    GWL_EXSTYLE,
};

const WM_KYNX_FRAME: u32 = WM_USER + 42;
const WM_KYNX_VISIBILITY: u32 = WM_USER + 43;
const WM_KYNX_RESIZE: u32 = WM_USER + 44;
const WM_KYNX_PLACE: u32 = WM_USER + 45;

#[derive(Clone)]
struct FrameBuffer {
    width: u32,
    height: u32,
    pixels: Arc<[u8]>,
}

struct CachedDib {
    width: u32,
    height: u32,
    hdc_mem: HDC,
    hbmp: HBITMAP,
    bits: *mut u8,
}

// SAFETY: only touched on the share-window UI thread.
unsafe impl Send for CachedDib {}

struct WindowState {
    frame: Mutex<Option<FrameBuffer>>,
    dib: Mutex<Option<CachedDib>>,
}

pub struct ShareWindow {
    hwnd: HWND,
    state: Arc<WindowState>,
    running: Arc<AtomicBool>,
    closed: AtomicBool,
    last_size: AtomicU64,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

unsafe impl Send for ShareWindow {}
unsafe impl Sync for ShareWindow {}

impl ShareWindow {
    pub fn create(cfg: ShareWindowConfig) -> Result<Self> {
        let running = Arc::new(AtomicBool::new(true));
        let (tx, rx) = std::sync::mpsc::channel::<Result<(isize, Arc<WindowState>)>>();
        let running_clone = Arc::clone(&running);
        let title = cfg.title.clone();
        let always_on_top = cfg.always_on_top;
        let visible = cfg.visible;

        let thread = std::thread::Builder::new()
            .name("kynx-share-window".into())
            .spawn(move || match unsafe { create_window(&title, always_on_top, visible) } {
                Ok((hwnd_bits, state)) => {
                    let hwnd = HWND(hwnd_bits as *mut _);
                    let _ = tx.send(Ok((hwnd_bits, Arc::clone(&state))));
                    unsafe { message_loop(hwnd, running_clone) };
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            })
            .context("spawn share window thread")?;

        let (hwnd_bits, state) = rx.recv().context("share window init")??;
        let hwnd = HWND(hwnd_bits as *mut _);

        Ok(Self {
            hwnd,
            state,
            running,
            closed: AtomicBool::new(false),
            last_size: AtomicU64::new(0),
            thread: Mutex::new(Some(thread)),
        })
    }

    pub fn present(&self, width: u32, height: u32, bgra: &[u8]) -> Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        let expected = (width * height * 4) as usize;
        if bgra.len() < expected {
            return Err(anyhow!("frame buffer too small"));
        }

        // Prefer Arc clone when caller already has Arc<[u8]> via transmute path —
        // here we still need an owned Arc from the slice.
        let pixels: Arc<[u8]> = Arc::from(&bgra[..expected]);

        *self.state.frame.lock() = Some(FrameBuffer {
            width,
            height,
            pixels,
        });

        let packed_size = ((width as u64) << 32) | (height as u64);
        let prev = self.last_size.swap(packed_size, Ordering::Relaxed);
        unsafe {
            if prev != packed_size {
                let packed =
                    ((width.min(0xFFFF) as isize) << 16) | (height.min(0xFFFF) as isize);
                let _ = PostMessageW(Some(self.hwnd), WM_KYNX_RESIZE, WPARAM(0), LPARAM(packed));
            }
            let _ = PostMessageW(Some(self.hwnd), WM_KYNX_FRAME, WPARAM(0), LPARAM(0));
        }
        Ok(())
    }

    /// Zero-copy present when pixels are already Arc-backed.
    pub fn present_arc(&self, width: u32, height: u32, pixels: Arc<[u8]>) -> Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        let expected = (width * height * 4) as usize;
        if pixels.len() < expected {
            return Err(anyhow!("frame buffer too small"));
        }
        *self.state.frame.lock() = Some(FrameBuffer {
            width,
            height,
            pixels,
        });
        let packed_size = ((width as u64) << 32) | (height as u64);
        let prev = self.last_size.swap(packed_size, Ordering::Relaxed);
        unsafe {
            if prev != packed_size {
                let packed =
                    ((width.min(0xFFFF) as isize) << 16) | (height.min(0xFFFF) as isize);
                let _ = PostMessageW(Some(self.hwnd), WM_KYNX_RESIZE, WPARAM(0), LPARAM(packed));
            }
            let _ = PostMessageW(Some(self.hwnd), WM_KYNX_FRAME, WPARAM(0), LPARAM(0));
        }
        Ok(())
    }

    /// Place window borderless fullscreen on a monitor rect (for virtual display / Discord Screen).
    pub fn place_on_monitor(&self, x: i32, y: i32, width: u32, height: u32) {
        unsafe {
            // Pack x,y in lparam (16-bit each) — widths go via wparam high/low
            let packed_xy =
                ((x.clamp(-32768, 32767) as i16 as isize) << 16) | (y.clamp(-32768, 32767) as i16 as isize & 0xFFFF);
            let packed_wh = ((width.min(0xFFFF) as isize) << 16) | (height.min(0xFFFF) as isize);
            let _ = PostMessageW(
                Some(self.hwnd),
                WM_KYNX_PLACE,
                WPARAM(packed_wh as usize),
                LPARAM(packed_xy),
            );
        }
    }

    pub fn set_visible(&self, visible: bool) {
        unsafe {
            let _ = PostMessageW(
                Some(self.hwnd),
                WM_KYNX_VISIBILITY,
                WPARAM(if visible { 1 } else { 0 }),
                LPARAM(0),
            );
        }
    }

    /// Show window so Discord / OBS can pick it as a Window source.
    pub fn show_for_capture(&self) {
        self.set_visible(true);
    }

    pub fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.running.store(false, Ordering::SeqCst);
        unsafe {
            let _ = PostMessageW(Some(self.hwnd), WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(t) = self.thread.lock().take() {
            let _ = t.join();
        }
    }
}

impl Drop for ShareWindow {
    fn drop(&mut self) {
        self.close();
    }
}

unsafe fn create_window(
    title: &str,
    always_on_top: bool,
    visible: bool,
) -> Result<(isize, Arc<WindowState>)> {
    let class_name = windows::core::w!("KynxShareOutputClass");
    let hinstance = GetModuleHandleW(None)?;

    let black = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00121214));
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance.into(),
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        hbrBackground: black,
        lpszClassName: class_name,
        ..Default::default()
    };
    let _ = RegisterClassW(&wc);

    let title_wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    // Normal app window — Discord/OBS enumerate these. Avoid TOOLWINDOW (hidden from pickers).
    let mut ex = WS_EX_APPWINDOW;
    if always_on_top {
        ex |= WS_EX_TOPMOST;
    }

    let state = Arc::new(WindowState {
        frame: Mutex::new(None),
        dib: Mutex::new(None),
    });

    let hwnd = CreateWindowExW(
        ex,
        class_name,
        PCWSTR(title_wide.as_ptr()),
        WS_OVERLAPPEDWINDOW,
        80,
        80,
        1280,
        720,
        None,
        None,
        Some(hinstance.into()),
        None,
    )?;

    let raw = Box::into_raw(Box::new(Arc::clone(&state)));
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw as isize);

    if always_on_top {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
    }

    let _ = ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE });
    if visible {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
    Ok((hwnd.0 as isize, state))
}

unsafe fn message_loop(hwnd: HWND, running: Arc<AtomicBool>) {
    let mut msg = MSG::default();
    while running.load(Ordering::SeqCst) {
        let mut had_msg = false;
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            had_msg = true;
            if msg.message == WM_QUIT {
                running.store(false, Ordering::SeqCst);
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if !had_msg {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    let _ = DestroyWindow(hwnd);
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_DESTROY => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if ptr != 0 {
                let state = Box::from_raw(ptr as *mut Arc<WindowState>);
                if let Some(dib) = state.dib.lock().take() {
                    free_dib(dib);
                }
                drop(state);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            LRESULT(0)
        }
        WM_KYNX_VISIBILITY => {
            let show = wparam.0 != 0;
            let _ = ShowWindow(hwnd, if show { SW_SHOW } else { SW_HIDE });
            if show {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_SHOWWINDOW,
                );
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_KYNX_RESIZE => {
            let w = ((lparam.0 >> 16) & 0xFFFF) as i32;
            let h = (lparam.0 & 0xFFFF) as i32;
            if w > 16 && h > 16 {
                // Outer window size ≈ client + chrome (only when not in borderless place mode)
                let ow = (w + 16).clamp(320, 7680);
                let oh = (h + 40).clamp(240, 4320);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    ow,
                    oh,
                    SWP_NOMOVE | SWP_NOZORDER,
                );
            }
            LRESULT(0)
        }
        WM_KYNX_PLACE => {
            let w = ((wparam.0 >> 16) & 0xFFFF) as i32;
            let h = (wparam.0 & 0xFFFF) as i32;
            let x = ((lparam.0 >> 16) as i16) as i32;
            let y = (lparam.0 as i16) as i32;
            if w > 16 && h > 16 {
                // Borderless fullscreen on target monitor — Discord Screen capture sees full desktop.
                let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, WS_POPUP.0 as isize);
                let _ = SetWindowLongPtrW(
                    hwnd,
                    GWL_EXSTYLE,
                    (WS_EX_APPWINDOW | WS_EX_TOPMOST).0 as isize,
                );
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    x,
                    y,
                    w,
                    h,
                    SWP_FRAMECHANGED | SWP_SHOWWINDOW,
                );
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_KYNX_FRAME | WM_PAINT => {
            paint_frame(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn free_dib(dib: CachedDib) {
    let _ = DeleteObject(dib.hbmp.into());
    let _ = DeleteDC(dib.hdc_mem);
}

unsafe fn ensure_dib(state: &WindowState, hdc: HDC, width: u32, height: u32) -> Option<*mut u8> {
    {
        let dib = state.dib.lock();
        if let Some(d) = dib.as_ref() {
            if d.width == width && d.height == height && !d.bits.is_null() {
                return Some(d.bits);
            }
        }
    }

    if let Some(old) = state.dib.lock().take() {
        free_dib(old);
    }

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let hdc_mem = CreateCompatibleDC(Some(hdc));
    let hbmp_result = CreateDIBSection(Some(hdc_mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
    let Ok(hbmp) = hbmp_result else {
        let _ = DeleteDC(hdc_mem);
        return None;
    };
    if bits.is_null() {
        let _ = DeleteObject(hbmp.into());
        let _ = DeleteDC(hdc_mem);
        return None;
    }

    let ptr = bits as *mut u8;
    *state.dib.lock() = Some(CachedDib {
        width,
        height,
        hdc_mem,
        hbmp,
        bits: ptr,
    });
    Some(ptr)
}

unsafe fn paint_frame(hwnd: HWND) {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if ptr == 0 {
        return;
    }
    let state = &*(ptr as *const Arc<WindowState>);
    // Cheap Arc clone — no pixel copy
    let frame = state.frame.lock().clone();

    let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);

    let Some(frame) = frame else {
        let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00121214));
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let _ = FillRect(hdc, &client, brush);
        let _ = DeleteObject(brush.into());
        let _ = EndPaint(hwnd, &ps);
        return;
    };

    let mut client = RECT::default();
    let _ = GetClientRect(hwnd, &mut client);
    let cw = (client.right - client.left).max(1);
    let ch = (client.bottom - client.top).max(1);

    let Some(bits) = ensure_dib(state, hdc, frame.width, frame.height) else {
        let _ = EndPaint(hwnd, &ps);
        return;
    };

    let size = (frame.width * frame.height * 4) as usize;
    std::ptr::copy_nonoverlapping(
        frame.pixels.as_ptr(),
        bits,
        size.min(frame.pixels.len()),
    );

    let dib_guard = state.dib.lock();
    let Some(dib) = dib_guard.as_ref() else {
        drop(dib_guard);
        let _ = EndPaint(hwnd, &ps);
        return;
    };

    let old = SelectObject(dib.hdc_mem, dib.hbmp.into());
    let _ = StretchBlt(
        hdc,
        0,
        0,
        cw,
        ch,
        Some(dib.hdc_mem),
        0,
        0,
        frame.width as i32,
        frame.height as i32,
        SRCCOPY,
    );
    let _ = SelectObject(dib.hdc_mem, old);
    drop(dib_guard);
    let _ = EndPaint(hwnd, &ps);
}
