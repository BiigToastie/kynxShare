//! Parsec Virtual Display Driver (IddCx) control — creates a real Windows monitor
//! that Discord lists under **Bildschirm / Screen** with performance options.

use anyhow::{anyhow, bail, Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualDisplayStatus {
    pub installed: bool,
    pub adapter_name: Option<String>,
    pub guidance: String,
    /// True when a Parsec VDD device is present and ready.
    #[serde(default)]
    pub driver_ok: bool,
    /// Active plug index while streaming to virtual display (if any).
    #[serde(default)]
    pub active_index: Option<i32>,
    /// Device name like `\\.\DISPLAYn` for the virtual monitor.
    #[serde(default)]
    pub monitor_device: Option<String>,
}

impl Default for VirtualDisplayStatus {
    fn default() -> Self {
        Self {
            installed: false,
            adapter_name: None,
            guidance: String::new(),
            driver_ok: false,
            active_index: None,
            monitor_device: None,
        }
    }
}

const INSTALL_URL: &str = "https://builds.parsec.app/vdd/parsec-vdd-0.41.0.0.exe";
const DOCS_URL: &str = "https://github.com/nomi-san/parsec-vdd";

pub fn detect_virtual_display_driver() -> VirtualDisplayStatus {
    #[cfg(windows)]
    {
        detect_windows()
    }
    #[cfg(not(windows))]
    {
        VirtualDisplayStatus {
            guidance: "Virtual Display ist nur unter Windows verfügbar.".into(),
            ..Default::default()
        }
    }
}

pub fn install_driver_guidance() -> String {
    format!(
        "Parsec Virtual Display Driver einmalig installieren (Admin):\n\
         1. Download: {INSTALL_URL}\n\
         2. Installer ausführen (oder /S für still).\n\
         3. PC ggf. neu starten.\n\
         Docs: {DOCS_URL}\n\
         Danach in kynxShare „Virtueller Bildschirm“ aktivieren und Stream starten — \
         Discord → Bildschirm teilen → neuer Monitor mit Performance-Optionen."
    )
}

/// Returns true if this monitor looks like a virtual IDD (must not be captured).
pub fn is_virtual_monitor(name: &str, device_name: &str) -> bool {
    let hay = format!("{name} {device_name}").to_lowercase();
    hay.contains("parsecvda")
        || hay.contains("psccdd")
        || hay.contains("mttvdd")
        || hay.contains("virtual display")
        || hay.contains("idd sample")
        || hay.contains("usb-mobile-app")
        || hay.contains("virtualdesk")
}

/// Session that keeps one Parsec virtual monitor plugged while streaming.
pub struct VirtualDisplaySession {
    #[cfg(windows)]
    inner: Mutex<Option<WindowsVdd>>,
    #[cfg(not(windows))]
    _pad: (),
    ping_stop: Arc<AtomicBool>,
    ping_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    pub index: i32,
    pub monitor_device: Option<String>,
    pub monitor_x: i32,
    pub monitor_y: i32,
    pub monitor_w: u32,
    pub monitor_h: u32,
}

// HANDLE is only touched from the ping thread + stop — treat as Send/Sync like ShareWindow.
unsafe impl Send for VirtualDisplaySession {}
unsafe impl Sync for VirtualDisplaySession {}

impl VirtualDisplaySession {
    pub fn start(width: u32, height: u32, refresh_hz: u32) -> Result<Self> {
        #[cfg(windows)]
        {
            Self::start_windows(width, height, refresh_hz)
        }
        #[cfg(not(windows))]
        {
            let _ = (width, height, refresh_hz);
            bail!("Virtual Display nur unter Windows");
        }
    }

    pub fn stop(self) {
        self.ping_stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.ping_thread.lock().take() {
            let _ = t.join();
        }
        #[cfg(windows)]
        {
            if let Some(mut vdd) = self.inner.lock().take() {
                vdd.remove_display(self.index);
                vdd.close();
            }
        }
    }
}

#[cfg(windows)]
impl VirtualDisplaySession {
    fn start_windows(width: u32, height: u32, refresh_hz: u32) -> Result<Self> {
        let status = detect_windows();
        if !status.driver_ok {
            bail!("{}", install_driver_guidance());
        }

        let mut vdd = WindowsVdd::open().context("Parsec VDD öffnen")?;
        let index = vdd.add_display().context("Virtuellen Monitor hinzufügen")?;
        if index < 0 || index > 15 {
            vdd.close();
            bail!("VDD add display failed (index={index}). Ist der Treiber installiert?");
        }
        info!("Parsec VDD display plugged index={index}");

        // Keepalive must run <100ms or the driver unplugs all displays.
        let ping_stop = Arc::new(AtomicBool::new(false));
        let handle_bits = vdd.handle_bits();
        let ping_flag = Arc::clone(&ping_stop);
        let ping_thread = std::thread::Builder::new()
            .name("kynx-vdd-ping".into())
            .spawn(move || {
                while !ping_flag.load(Ordering::SeqCst) {
                    unsafe {
                        let h = windows::Win32::Foundation::HANDLE(handle_bits as *mut _);
                        let _ = vdd_ioctl(h, VDD_IOCTL_UPDATE, &[]);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            })
            .ok();

        // Wait for Windows to enumerate the new monitor.
        std::thread::sleep(Duration::from_millis(400));

        let mut monitor_device = find_parsec_monitor_device();
        for _ in 0..10 {
            if monitor_device.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
            monitor_device = find_parsec_monitor_device();
        }

        if let Some(ref device) = monitor_device {
            let _ = set_display_mode(device, width.max(640), height.max(360), refresh_hz.max(60));
            std::thread::sleep(Duration::from_millis(200));
        } else {
            warn!("Parsec monitor device not found yet — Discord may still see a default mode");
        }

        let (mx, my, mw, mh) = monitor_device
            .as_deref()
            .and_then(query_monitor_rect)
            .unwrap_or((0, 0, width.max(640), height.max(360)));

        Ok(Self {
            inner: Mutex::new(Some(vdd)),
            ping_stop,
            ping_thread: Mutex::new(ping_thread),
            index,
            monitor_device,
            monitor_x: mx,
            monitor_y: my,
            monitor_w: mw,
            monitor_h: mh,
        })
    }
}

#[cfg(windows)]
fn detect_windows() -> VirtualDisplayStatus {
    use windows::core::GUID;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};

    let guidance = install_driver_guidance();
    let driver_ok = query_parsec_driver_ok();

    // Also detect adapter name via DXGI / device.
    let mut adapter_name = if driver_ok {
        Some("Parsec Virtual Display Adapter".into())
    } else {
        None
    };

    unsafe {
        if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() {
            let mut index = 0u32;
            loop {
                let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(index) {
                    Ok(a) => a,
                    Err(_) => break,
                };
                if let Ok(desc) = adapter.GetDesc1() {
                    let name = wchar_to_string(&desc.Description);
                    let lower = name.to_lowercase();
                    if lower.contains("parsec")
                        || lower.contains("mttvdd")
                        || lower.contains("virtual display")
                        || lower.contains("idd sample")
                    {
                        adapter_name = Some(name);
                        break;
                    }
                }
                index += 1;
            }
        }
    }

    let installed = driver_ok || adapter_name.is_some();
    let _ = GUID::from_u128(0x00b41627_04c4_429e_a26e_0265cf50c8fa); // keep GUID referenced

    VirtualDisplayStatus {
        installed,
        adapter_name,
        guidance: if driver_ok {
            "Parsec VDD bereit. Aktiviere „Virtueller Bildschirm“ und starte den Stream — \
             Discord → Bildschirm → neuer Monitor (Performance-Optionen verfügbar)."
                .into()
        } else {
            guidance
        },
        driver_ok,
        active_index: None,
        monitor_device: None,
    }
}

#[cfg(windows)]
fn query_parsec_driver_ok() -> bool {
    match WindowsVdd::open() {
        Ok(mut v) => {
            v.close();
            true
        }
        Err(_) => false,
    }
}

#[cfg(windows)]
const VDD_IOCTL_ADD: u32 = 0x0022e004;
#[cfg(windows)]
const VDD_IOCTL_REMOVE: u32 = 0x0022a008;
#[cfg(windows)]
const VDD_IOCTL_UPDATE: u32 = 0x0022a00c;

#[cfg(windows)]
struct WindowsVdd {
    /// Raw HANDLE bits — kept as isize so the session is Send/Sync for Tauri.
    handle_bits: isize,
}

#[cfg(windows)]
impl WindowsVdd {
    fn open() -> Result<Self> {
        unsafe {
            let guid = windows::core::GUID::from_u128(0x00b41627_04c4_429e_a26e_0265cf50c8fa);
            let handle = open_device_handle(&guid)?;
            Ok(Self {
                handle_bits: handle.0 as isize,
            })
        }
    }

    fn handle(&self) -> windows::Win32::Foundation::HANDLE {
        windows::Win32::Foundation::HANDLE(self.handle_bits as *mut _)
    }

    fn handle_bits(&self) -> isize {
        self.handle_bits
    }

    fn add_display(&mut self) -> Result<i32> {
        unsafe {
            let idx = vdd_ioctl(self.handle(), VDD_IOCTL_ADD, &[])?;
            let _ = vdd_ioctl(self.handle(), VDD_IOCTL_UPDATE, &[]);
            Ok(idx as i32)
        }
    }

    fn remove_display(&mut self, index: i32) {
        unsafe {
            let be: u16 = (((index & 0xFF) as u16) << 8) | (((index >> 8) & 0xFF) as u16);
            let bytes = be.to_ne_bytes();
            let _ = vdd_ioctl(self.handle(), VDD_IOCTL_REMOVE, &bytes);
            let _ = vdd_ioctl(self.handle(), VDD_IOCTL_UPDATE, &[]);
        }
    }

    fn close(&mut self) {
        if self.handle_bits != 0 {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(self.handle());
            }
            self.handle_bits = 0;
        }
    }
}

#[cfg(windows)]
unsafe fn open_device_handle(interface_guid: &windows::core::GUID) -> Result<windows::Win32::Foundation::HANDLE> {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
        SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT,
        SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
    };
    use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_NO_BUFFERING, FILE_FLAG_OVERLAPPED, FILE_FLAG_WRITE_THROUGH,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL,
    };

    let dev_info = SetupDiGetClassDevsW(
        Some(interface_guid),
        None,
        None,
        DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
    )?;

    let mut result = Err(anyhow!("Parsec VDD device interface not found — Treiber installieren"));
    let mut index = 0u32;
    loop {
        let mut iface = SP_DEVICE_INTERFACE_DATA {
            cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..Default::default()
        };
        if SetupDiEnumDeviceInterfaces(dev_info, None, interface_guid, index, &mut iface).is_err() {
            break;
        }

        let mut required = 0u32;
        let _ = SetupDiGetDeviceInterfaceDetailW(
            dev_info,
            &iface,
            None,
            0,
            Some(&mut required),
            None,
        );
        if required == 0 {
            index += 1;
            continue;
        }

        let mut buf = vec![0u8; required as usize];
        let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
        (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;

        if SetupDiGetDeviceInterfaceDetailW(
            dev_info,
            &iface,
            Some(detail),
            required,
            None,
            None,
        )
        .is_ok()
        {
            let path_ptr = std::ptr::addr_of!((*detail).DevicePath) as *const u16;
            let path = windows::core::PCWSTR(path_ptr);
            match CreateFileW(
                path,
                (GENERIC_READ.0 | GENERIC_WRITE.0) as u32,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_NO_BUFFERING | FILE_FLAG_OVERLAPPED | FILE_FLAG_WRITE_THROUGH,
                None,
            ) {
                Ok(h) if h != INVALID_HANDLE_VALUE => {
                    result = Ok(h);
                    break;
                }
                _ => {}
            }
        }
        index += 1;
    }

    let _ = SetupDiDestroyDeviceInfoList(dev_info);
    result
}

#[cfg(windows)]
unsafe fn vdd_ioctl(handle: windows::Win32::Foundation::HANDLE, code: u32, data: &[u8]) -> Result<u32> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::IO::{DeviceIoControl, GetOverlappedResultEx};
    use windows::Win32::System::Threading::CreateEventW;

    let mut in_buf = [0u8; 32];
    let n = data.len().min(32);
    in_buf[..n].copy_from_slice(&data[..n]);

    let event = CreateEventW(None, true, false, None)?;
    let mut overlapped = windows::Win32::System::IO::OVERLAPPED {
        hEvent: event,
        ..Default::default()
    };
    let mut out: u32 = 0;
    let _ = DeviceIoControl(
        handle,
        code,
        Some(in_buf.as_ptr() as *const _),
        32,
        Some(&mut out as *mut u32 as *mut _),
        4,
        None,
        Some(&mut overlapped),
    );

    let mut transferred = 0u32;
    let ok = GetOverlappedResultEx(handle, &overlapped, &mut transferred, 5000, false);
    let _ = CloseHandle(event);
    if ok.is_err() {
        return Err(anyhow!("VDD IOCTL 0x{code:x} failed"));
    }
    Ok(out)
}

#[cfg(windows)]
fn find_parsec_monitor_device() -> Option<String> {
    use windows::Win32::Graphics::Gdi::{EnumDisplayDevicesW, DISPLAY_DEVICEW};

    unsafe {
        let mut i = 0u32;
        loop {
            let mut dd = DISPLAY_DEVICEW {
                cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            if !EnumDisplayDevicesW(None, i, &mut dd, 0).as_bool() {
                break;
            }
            let name = wchar_to_string(&dd.DeviceName);
            let string = wchar_to_string(&dd.DeviceString);
            let id = wchar_to_string(&dd.DeviceID);
            if is_virtual_monitor(&string, &id)
                || string.to_lowercase().contains("parsec")
                || id.to_lowercase().contains("psccdd")
            {
                return Some(name);
            }
            i += 1;
            if i > 32 {
                break;
            }
        }
    }
    None
}

#[cfg(windows)]
fn set_display_mode(device: &str, width: u32, height: u32, refresh_hz: u32) -> Result<()> {
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        ChangeDisplaySettingsExW, EnumDisplaySettingsW, CDS_UPDATEREGISTRY, DEVMODEW,
        ENUM_CURRENT_SETTINGS, DISP_CHANGE_SUCCESSFUL,
    };

    let wide: Vec<u16> = device.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut mode = DEVMODEW::default();
        mode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        if !EnumDisplaySettingsW(PCWSTR(wide.as_ptr()), ENUM_CURRENT_SETTINGS, &mut mode).as_bool()
        {
            bail!("EnumDisplaySettingsW failed for {device}");
        }
        mode.dmPelsWidth = width;
        mode.dmPelsHeight = height;
        mode.dmDisplayFrequency = refresh_hz;
        mode.dmFields |= windows::Win32::Graphics::Gdi::DM_PELSWIDTH
            | windows::Win32::Graphics::Gdi::DM_PELSHEIGHT
            | windows::Win32::Graphics::Gdi::DM_DISPLAYFREQUENCY;

        let rc = ChangeDisplaySettingsExW(
            PCWSTR(wide.as_ptr()),
            Some(&mode),
            None,
            CDS_UPDATEREGISTRY,
            None,
        );
        if rc != DISP_CHANGE_SUCCESSFUL {
            // Fallback: try without forcing refresh
            mode.dmDisplayFrequency = 60;
            let rc2 = ChangeDisplaySettingsExW(
                PCWSTR(wide.as_ptr()),
                Some(&mode),
                None,
                CDS_UPDATEREGISTRY,
                None,
            );
            if rc2 != DISP_CHANGE_SUCCESSFUL {
                warn!("ChangeDisplaySettingsExW for {device} -> {rc:?}/{rc2:?}");
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn query_monitor_rect(device: &str) -> Option<(i32, i32, u32, u32)> {
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{EnumDisplaySettingsW, DEVMODEW, ENUM_CURRENT_SETTINGS};

    let wide: Vec<u16> = device.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut mode = DEVMODEW::default();
        mode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        if !EnumDisplaySettingsW(PCWSTR(wide.as_ptr()), ENUM_CURRENT_SETTINGS, &mut mode).as_bool()
        {
            return None;
        }
        let x = mode.Anonymous1.Anonymous2.dmPosition.x;
        let y = mode.Anonymous1.Anonymous2.dmPosition.y;
        Some((x, y, mode.dmPelsWidth, mode.dmPelsHeight))
    }
}

#[cfg(windows)]
fn wchar_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Open the Parsec VDD installer download in the default browser.
pub fn open_driver_installer() -> Result<()> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let url: Vec<u16> = INSTALL_URL.encode_utf16().chain(std::iter::once(0)).collect();
        let op: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let ret = ShellExecuteW(
                None,
                PCWSTR(op.as_ptr()),
                PCWSTR(url.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            );
            if (ret.0 as isize) <= 32 {
                bail!("ShellExecute failed");
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        bail!("nur Windows");
    }
}
