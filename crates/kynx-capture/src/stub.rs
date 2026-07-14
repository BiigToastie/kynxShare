use crate::monitor::MonitorInfo;
use crate::types::{CaptureError, CapturedFrame, FramePixelFormat};
use anyhow::Result;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct CaptureSession {
    monitor: MonitorInfo,
}

impl CaptureSession {
    pub fn open(monitor: MonitorInfo) -> Result<Self> {
        Ok(Self { monitor })
    }

    pub fn capture(&self, _timeout_ms: u32) -> Result<CapturedFrame, CaptureError> {
        Ok(stub_frame(&self.monitor))
    }

    pub fn monitor(&self) -> MonitorInfo {
        self.monitor.clone()
    }
}

pub struct MultiCapture {
    sessions: Vec<Arc<CaptureSession>>,
    running: Arc<AtomicBool>,
    latest: Arc<Mutex<HashMap<String, CapturedFrame>>>,
    handles: Mutex<Vec<std::thread::JoinHandle<()>>>,
}

impl MultiCapture {
    pub fn start(monitors: Vec<MonitorInfo>) -> Result<Self> {
        let sessions: Vec<_> = monitors
            .into_iter()
            .map(|m| Arc::new(CaptureSession::open(m).unwrap()))
            .collect();
        let latest = Arc::new(Mutex::new(HashMap::new()));
        let running = Arc::new(AtomicBool::new(true));
        let mut handles = Vec::new();
        for session in &sessions {
            let session = Arc::clone(session);
            let latest = Arc::clone(&latest);
            let running = Arc::clone(&running);
            handles.push(std::thread::spawn(move || {
                while running.load(Ordering::SeqCst) {
                    if let Ok(frame) = session.capture(16) {
                        latest.lock().insert(frame.monitor_id.clone(), frame);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(33));
                }
            }));
        }
        Ok(Self {
            sessions,
            running,
            latest,
            handles: Mutex::new(handles),
        })
    }

    pub fn snapshot(&self) -> HashMap<String, CapturedFrame> {
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

fn stub_frame(monitor: &MonitorInfo) -> CapturedFrame {
    let w = monitor.width;
    let h = monitor.height;
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            pixels[i] = ((x * 255) / w.max(1)) as u8;
            pixels[i + 1] = ((y * 255) / h.max(1)) as u8;
            pixels[i + 2] = 80;
            pixels[i + 3] = 255;
        }
    }
    CapturedFrame {
        monitor_id: monitor.id.clone(),
        width: w,
        height: h,
        stride: w * 4,
        format: FramePixelFormat::Bgra8,
        pixels: pixels.into(),
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    }
}
