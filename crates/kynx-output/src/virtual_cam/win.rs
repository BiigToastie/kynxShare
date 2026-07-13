use super::{
    VirtualCamHeader, VirtualCameraConfig, VIRTUAL_CAM_HEADER_SIZE, VIRTUAL_CAM_MAGIC,
    VIRTUAL_CAM_MAPPING_NAME,
};
use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::PCWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS, PAGE_READWRITE,
    MEMORY_MAPPED_VIEW_ADDRESS,
};
use windows::Win32::Foundation::HANDLE;

/// Max payload for 4K BGRA + header
const MAX_MAP_SIZE: usize = VIRTUAL_CAM_HEADER_SIZE + 3840 * 2160 * 4;

pub struct VirtualCamera {
    enabled: AtomicBool,
    closed: AtomicBool,
    handle: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    max_w: u32,
    max_h: u32,
    frame_id: Mutex<u64>,
}

unsafe impl Send for VirtualCamera {}
unsafe impl Sync for VirtualCamera {}

impl VirtualCamera {
    pub fn open(cfg: VirtualCameraConfig) -> Result<Self> {
        unsafe {
            let mut name: Vec<u16> = VIRTUAL_CAM_MAPPING_NAME
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let handle = CreateFileMappingW(
                windows::Win32::Foundation::INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                MAX_MAP_SIZE as u32,
                PCWSTR(name.as_mut_ptr()),
            )
            .context("CreateFileMappingW for virtual camera")?;

            let view = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, MAX_MAP_SIZE);
            if view.Value.is_null() {
                let _ = CloseHandle(handle);
                anyhow::bail!("MapViewOfFile failed for virtual camera");
            }

            // Zero header
            std::ptr::write_bytes(view.Value as *mut u8, 0, VIRTUAL_CAM_HEADER_SIZE);

            Ok(Self {
                enabled: AtomicBool::new(cfg.enabled),
                closed: AtomicBool::new(false),
                handle,
                view,
                max_w: cfg.max_width,
                max_h: cfg.max_height,
                frame_id: Mutex::new(0),
            })
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    pub fn push_frame(&self, width: u32, height: u32, bgra: &[u8], timestamp_ms: u64) {
        if !self.enabled.load(Ordering::SeqCst) {
            return;
        }
        let w = width.min(self.max_w);
        let h = height.min(self.max_h);
        if w == 0 || h == 0 {
            return;
        }
        // If larger than max, skip (caller should downscale)
        if width > self.max_w || height > self.max_h {
            return;
        }
        let stride = w * 4;
        let data_size = (stride * h) as usize;
        if bgra.len() < data_size {
            return;
        }
        let mut id = self.frame_id.lock();
        *id = id.wrapping_add(1);
        let frame_id = *id;

        unsafe {
            let base = self.view.Value as *mut u8;
            let header = VirtualCamHeader {
                magic: VIRTUAL_CAM_MAGIC,
                version: 1,
                width: w,
                height: h,
                stride,
                format: 0,
                frame_id,
                timestamp_ms,
                data_offset: VIRTUAL_CAM_HEADER_SIZE as u32,
                data_size: data_size as u32,
            };
            std::ptr::copy_nonoverlapping(
                &header as *const _ as *const u8,
                base,
                std::mem::size_of::<VirtualCamHeader>(),
            );
            std::ptr::copy_nonoverlapping(
                bgra.as_ptr(),
                base.add(VIRTUAL_CAM_HEADER_SIZE),
                data_size,
            );
        }
    }

    pub fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        unsafe {
            let _ = UnmapViewOfFile(self.view);
            let _ = CloseHandle(self.handle);
        }
    }
}

impl Drop for VirtualCamera {
    fn drop(&mut self) {
        self.close();
    }
}
