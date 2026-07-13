use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualDisplayStatus {
    pub installed: bool,
    pub adapter_name: Option<String>,
    pub guidance: String,
}

/// Detect whether Virtual Display Driver (or similar IDD) appears present.
pub fn detect_virtual_display_driver() -> VirtualDisplayStatus {
    #[cfg(windows)]
    {
        detect_windows()
    }
    #[cfg(not(windows))]
    {
        VirtualDisplayStatus {
            installed: false,
            adapter_name: None,
            guidance: "Virtual Display Driver is only available on Windows.".into(),
        }
    }
}

#[cfg(windows)]
fn detect_windows() -> VirtualDisplayStatus {
    use windows::Win32::Graphics::Dxgi::CreateDXGIFactory1;
    use windows::Win32::Graphics::Dxgi::IDXGIAdapter1;
    use windows::Win32::Graphics::Dxgi::IDXGIFactory1;

    let guidance = "Install Virtual Display Driver (MIT, signed) from \
        https://github.com/VirtualDrivers/Virtual-Display-Driver — then set its resolution \
        to match your kynxShare canvas and share that screen in Discord."
        .to_string();

    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return VirtualDisplayStatus {
                installed: false,
                adapter_name: None,
                guidance,
            };
        };

        let mut index = 0u32;
        loop {
            let adapter: IDXGIAdapter1 = match factory.EnumAdapters1(index) {
                Ok(a) => a,
                Err(_) => break,
            };
            if let Ok(desc) = adapter.GetDesc1() {
                let name = wchar_to_string(&desc.Description);
                let lower = name.to_lowercase();
                if lower.contains("mttvdd")
                    || lower.contains("virtual display")
                    || lower.contains("idd sample")
                    || lower.contains("usb-mobile-app")
                {
                    return VirtualDisplayStatus {
                        installed: true,
                        adapter_name: Some(name),
                        guidance: "Virtual display adapter detected. Set its resolution to your \
                            kynxShare output size, then share that screen in Discord."
                            .into(),
                    };
                }
            }
            index += 1;
        }
    }

    VirtualDisplayStatus {
        installed: false,
        adapter_name: None,
        guidance,
    }
}

#[cfg(windows)]
fn wchar_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}
