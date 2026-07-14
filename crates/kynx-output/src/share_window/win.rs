use super::ShareWindowConfig;
use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateCompatibleDC, CreateDIBSection, CreateSolidBrush, DeleteDC, DeleteObject,
    EndPaint, FillRect, SelectObject, StretchBlt, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
    GetWindowLongPtrW, LoadCursorW, PeekMessageW, PostMessageW, RegisterClassW, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA,
    IDC_ARROW, MSG, PM_REMOVE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, WM_DESTROY,
    WM_ERASEBKGND, WM_PAINT, WM_QUIT, WM_USER, WNDCLASSW, WS_EX_APPWINDOW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW, HWND_TOPMOST,
};

const WM_KYNX_FRAME: u32 = WM_USER + 42;
const WM_KYNX_VISIBILITY: u32 = WM_USER + 43;

#[derive(Clone)]
struct FrameBuffer {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

struct WindowState {
    frame: Mutex<Option<FrameBuffer>>,
}

pub struct ShareWindow {
    hwnd: HWND,
    state: Arc<WindowState>,
    running: Arc<AtomicBool>,
    closed: AtomicBool,
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
        *self.state.frame.lock() = Some(FrameBuffer {
            width,
            height,
            pixels: bgra[..expected].to_vec(),
        });
        unsafe {
            let _ = PostMessageW(Some(self.hwnd), WM_KYNX_FRAME, WPARAM(0), LPARAM(0));
        }
        Ok(())
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
    let mut ex = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_APPWINDOW;
    if always_on_top {
        ex |= WS_EX_TOPMOST;
    }

    let state = Arc::new(WindowState {
        frame: Mutex::new(None),
    });

    let hwnd = CreateWindowExW(
        ex,
        class_name,
        PCWSTR(title_wide.as_ptr()),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
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

    // Always start hidden unless explicitly requested — avoids white flash on launch
    let _ = ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE });
    Ok((hwnd.0 as isize, state))
}

unsafe fn message_loop(hwnd: HWND, running: Arc<AtomicBool>) {
    let mut msg = MSG::default();
    while running.load(Ordering::SeqCst) {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                running.store(false, Ordering::SeqCst);
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
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
                drop(Box::from_raw(ptr as *mut Arc<WindowState>));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            LRESULT(0)
        }
        WM_KYNX_VISIBILITY => {
            let _ = ShowWindow(hwnd, if wparam.0 != 0 { SW_SHOW } else { SW_HIDE });
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

unsafe fn paint_frame(hwnd: HWND) {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if ptr == 0 {
        return;
    }
    let state = &*(ptr as *const Arc<WindowState>);
    let frame = state.frame.lock().clone();

    let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);

    let Some(frame) = frame else {
        // Fill dark so the window never flashes white
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

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: frame.width as i32,
            biHeight: -(frame.height as i32),
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

    if let Ok(hbmp) = hbmp_result {
        if !bits.is_null() {
            let size = (frame.width * frame.height * 4) as usize;
            std::ptr::copy_nonoverlapping(
                frame.pixels.as_ptr(),
                bits as *mut u8,
                size.min(frame.pixels.len()),
            );
            let old = SelectObject(hdc_mem, hbmp.into());
            let _ = StretchBlt(
                hdc,
                0,
                0,
                cw,
                ch,
                Some(hdc_mem),
                0,
                0,
                frame.width as i32,
                frame.height as i32,
                SRCCOPY,
            );
            let _ = SelectObject(hdc_mem, old);
        }
        let _ = DeleteObject(hbmp.into());
    }
    let _ = DeleteDC(hdc_mem);
    let _ = EndPaint(hwnd, &ps);
}
