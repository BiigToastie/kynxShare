//! Parsec Virtual Display Driver helpers.
//!
//! SAFETY: Automatically plugging virtual monitors via IOCTL + ChangeDisplaySettingsEx
//! caused system instability (display reset / restart loops). **Plug/unplug is disabled.**
//! Detection remains read-only. Discord should use Fenster → kynxShare Output until a
//! safer VDD path exists.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use tracing::warn;

/// Hard kill-switch — never plug a virtual monitor from this process.
const ALLOW_VDD_PLUG: bool = false;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualDisplayStatus {
    pub installed: bool,
    pub adapter_name: Option<String>,
    pub guidance: String,
    #[serde(default)]
    pub driver_ok: bool,
    #[serde(default)]
    pub active_index: Option<i32>,
    #[serde(default)]
    pub monitor_device: Option<String>,
    /// True when auto-plug is disabled for stability.
    #[serde(default)]
    pub plug_disabled: bool,
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
            plug_disabled: true,
        }
    }
}

const INSTALL_URL: &str = "https://builds.parsec.app/vdd/parsec-vdd-0.41.0.0.exe";
const DOCS_URL: &str = "https://github.com/nomi-san/parsec-vdd";

static DETECT_CACHE: OnceLock<Mutex<(std::time::Instant, VirtualDisplayStatus)>> = OnceLock::new();
static VDD_PLUG_BLOCKED: AtomicBool = AtomicBool::new(true);

pub fn detect_virtual_display_driver() -> VirtualDisplayStatus {
    // Cache aggressively — never open device handles from the UI poll path.
    let cache = DETECT_CACHE.get_or_init(|| {
        Mutex::new((
            std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(60))
                .unwrap_or_else(std::time::Instant::now),
            VirtualDisplayStatus::default(),
        ))
    });
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if guard.0.elapsed() < std::time::Duration::from_secs(5) {
        let mut st = guard.1.clone();
        st.plug_disabled = true;
        return st;
    }
    let mut st = detect_readonly();
    st.plug_disabled = true;
    st.guidance = if ALLOW_VDD_PLUG && !VDD_PLUG_BLOCKED.load(Ordering::SeqCst) {
        st.guidance
    } else {
        "Virtueller-Bildschirm-Auto-Plug ist deaktiviert (Stabilität). \
         Discord: Fenster → „kynxShare Output“. \
         Optionalen Treiber kannst du separat installieren, kynxShare steckt keinen Monitor mehr automatisch."
            .into()
    };
    *guard = (std::time::Instant::now(), st.clone());
    st
}

pub fn install_driver_guidance() -> String {
    format!(
        "Parsec VDD optional: {INSTALL_URL} ({DOCS_URL}).\n\
         kynxShare steckt derzeit **keinen** virtuellen Monitor automatisch — \
         nutze Discord → Fenster → kynxShare Output."
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

/// Session stub — plug is disabled; start always fails closed.
pub struct VirtualDisplaySession {
    pub index: i32,
    pub monitor_device: Option<String>,
    pub monitor_x: i32,
    pub monitor_y: i32,
    pub monitor_w: u32,
    pub monitor_h: u32,
}

unsafe impl Send for VirtualDisplaySession {}
unsafe impl Sync for VirtualDisplaySession {}

impl VirtualDisplaySession {
    pub fn start(_width: u32, _height: u32, _refresh_hz: u32) -> Result<Self> {
        VDD_PLUG_BLOCKED.store(true, Ordering::SeqCst);
        warn!("VDD plug refused — disabled for system stability");
        bail!(
            "Virtueller Bildschirm ist absichtlich deaktiviert (Absturzschutz).\n\
             Bitte Discord → Fenster → „kynxShare Output“ verwenden."
        );
    }

    pub fn stop(self) {
        // No-op: we never successfully plug.
    }
}

fn detect_readonly() -> VirtualDisplayStatus {
    #[cfg(windows)]
    {
        detect_windows_dxgi_only()
    }
    #[cfg(not(windows))]
    {
        VirtualDisplayStatus {
            guidance: "Virtual Display ist nur unter Windows verfügbar.".into(),
            plug_disabled: true,
            ..Default::default()
        }
    }
}

/// DXGI adapter name scan only — never opens the Parsec device interface.
#[cfg(windows)]
fn detect_windows_dxgi_only() -> VirtualDisplayStatus {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};

    let mut adapter_name = None;
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

    let installed = adapter_name.is_some();
    VirtualDisplayStatus {
        installed,
        adapter_name,
        guidance: install_driver_guidance(),
        driver_ok: false, // never claim plug-ready while plug is disabled
        active_index: None,
        monitor_device: None,
        plug_disabled: true,
    }
}

#[cfg(windows)]
fn wchar_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

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
