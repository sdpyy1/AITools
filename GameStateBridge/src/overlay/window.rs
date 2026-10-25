use super::config::OverlayConfig;
use serde::Serialize;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const SPACE_KEY_DELAY: Duration = Duration::from_millis(350);
const FOCUS_SETTLE_DELAY: Duration = Duration::from_millis(60);
const BEFORE_MINIMIZE_DELAY: Duration = Duration::from_millis(120);

/// 用户自定义事件，用于从外部线程控制浏览器
#[derive(Debug, Clone)]
pub enum UserEvent {
    Show,
    Hide,
    UpdateUrl(String),
    Exit,
}

/// Overlay 调试快照
#[derive(Debug, Clone, Default, Serialize)]
pub struct OverlayDebugSnapshot {
    pub player_name: String,
    pub player_steam_id: Option<String>,
    pub detected_health: Option<i32>,
    pub is_alive: bool,
    pub source: String,
    pub matched_name: Option<String>,
    pub matched_steam_id: Option<String>,
    pub spectating: bool,
    pub visible: bool,
}

/// 浏览器弹窗运行时状态
pub struct OverlayWindowState {
    pub visible: bool,
    pub config: OverlayConfig,
    pub proxy: Option<Sender<UserEvent>>,
    /// 最新的调试信息（由 gsi_watcher 更新）
    pub debug: OverlayDebugSnapshot,
}

impl OverlayWindowState {
    pub fn new(config: OverlayConfig) -> Self {
        Self {
            visible: false,
            config,
            proxy: None,
            debug: OverlayDebugSnapshot::default(),
        }
    }
}

pub type SharedOverlayWindowState = Arc<Mutex<OverlayWindowState>>;

/// 在独立线程中运行外部 Chrome 控制器
pub fn run_overlay_thread(state: SharedOverlayWindowState) {
    std::thread::spawn(move || {
        run_browser_loop(state);
    });
}

fn run_browser_loop(shared_state: SharedOverlayWindowState) {
    let (tx, rx) = mpsc::channel::<UserEvent>();
    {
        let mut s = shared_state.lock().unwrap();
        s.proxy = Some(tx);
    }

    let mut controller = ChromeOverlayController::new(current_config(&shared_state));

    while let Ok(event) = rx.recv() {
        match event {
            UserEvent::Show => {
                controller.config = current_config(&shared_state);
                if let Err(e) = controller.show() {
                    tracing::error!("Failed to show Chrome overlay: {}", e);
                }
            }
            UserEvent::Hide => {
                controller.hide();
            }
            UserEvent::UpdateUrl(url) => {
                controller.config = current_config(&shared_state);
                controller.config.url = url;
            }
            UserEvent::Exit => {
                controller.hide();
                break;
            }
        }
    }
}

fn current_config(shared_state: &SharedOverlayWindowState) -> OverlayConfig {
    let s = shared_state.lock().unwrap();
    s.config.clone()
}

struct ChromeOverlayController {
    config: OverlayConfig,
    child: Option<Child>,
    pid: Option<u32>,
    generation: Arc<AtomicU64>,
    active_session: bool,
    pause_sent_for_session: bool,
}

impl ChromeOverlayController {
    fn new(config: OverlayConfig) -> Self {
        Self {
            config,
            child: None,
            pid: None,
            generation: Arc::new(AtomicU64::new(0)),
            active_session: false,
            pause_sent_for_session: false,
        }
    }

    fn show(&mut self) -> anyhow::Result<()> {
        self.clear_exited_child();

        self.active_session = true;
        self.pause_sent_for_session = false;

        if let Some(hwnd) = self.window_handle() {
            activate_window_preserving_state(hwnd);
            self.schedule_play_toggle();
            tracing::info!("Chrome overlay restored");
            return Ok(());
        }

        let chrome = find_chrome_exe().unwrap_or_else(|| PathBuf::from("chrome.exe"));
        let profile_dir = chrome_profile_dir();
        std::fs::create_dir_all(&profile_dir)?;

        let child = Command::new(&chrome)
            .arg("--new-window")
            .arg("--no-first-run")
            .arg("--disable-session-crashed-bubble")
            .arg("--autoplay-policy=no-user-gesture-required")
            .arg(format!("--user-data-dir={}", profile_dir.to_string_lossy()))
            .arg(&self.config.url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to launch Chrome at {}: {}",
                    chrome.to_string_lossy(),
                    e
                )
            })?;

        self.pid = Some(child.id());
        self.child = Some(child);
        self.schedule_play_toggle();
        tracing::info!("Chrome overlay opened: {}", self.config.url);
        Ok(())
    }

    fn hide(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.clear_exited_child();

        let Some(hwnd) = self.window_handle() else {
            return;
        };

        activate_window_preserving_state(hwnd);
        std::thread::sleep(FOCUS_SETTLE_DELAY);
        if self.active_session && !self.pause_sent_for_session {
            send_native_space_key();
            self.pause_sent_for_session = true;
        }
        std::thread::sleep(BEFORE_MINIMIZE_DELAY);
        minimize_window(hwnd);
        self.active_session = false;
        tracing::info!("Chrome overlay paused once and minimized");
    }

    fn schedule_play_toggle(&self) {
        let generation = self
            .generation
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1);
        schedule_single_space_key(self.generation.clone(), generation, self.pid);
    }

    fn window_handle(&self) -> Option<WindowHandle> {
        self.pid.and_then(find_main_window_by_pid)
    }

    fn clear_exited_child(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };

        match child.try_wait() {
            Ok(Some(_)) => {
                self.child = None;
                self.pid = None;
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("Failed to query Chrome overlay process: {}", e);
            }
        }
    }
}

fn schedule_single_space_key(generation: Arc<AtomicU64>, expected: u64, pid: Option<u32>) {
    std::thread::spawn(move || {
        std::thread::sleep(SPACE_KEY_DELAY);
        if generation.load(Ordering::SeqCst) != expected {
            return;
        }
        if let Some(hwnd) = pid.and_then(find_main_window_by_pid) {
            activate_window_preserving_state(hwnd);
            std::thread::sleep(FOCUS_SETTLE_DELAY);
        }
        if generation.load(Ordering::SeqCst) == expected {
            send_native_space_key();
        }
    });
}

fn chrome_profile_dir() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    exe_dir.join("chrome_overlay_profile")
}

fn find_chrome_exe() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(program_files) = std::env::var("ProgramFiles") {
        candidates.push(PathBuf::from(program_files).join("Google/Chrome/Application/chrome.exe"));
    }
    if let Ok(program_files_x86) = std::env::var("ProgramFiles(x86)") {
        candidates
            .push(PathBuf::from(program_files_x86).join("Google/Chrome/Application/chrome.exe"));
    }
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local_app_data).join("Google/Chrome/Application/chrome.exe"));
    }

    candidates.into_iter().find(|path| path.exists())
}

#[cfg(target_os = "windows")]
type WindowHandle = windows_sys::Win32::Foundation::HWND;

#[cfg(not(target_os = "windows"))]
type WindowHandle = ();

#[cfg(target_os = "windows")]
fn find_main_window_by_pid(pid: u32) -> Option<WindowHandle> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
    };
    use windows_sys::core::BOOL;

    struct FindWindowData {
        pid: u32,
        hwnd: HWND,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = unsafe { &mut *(lparam as *mut FindWindowData) };
        let mut window_pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut window_pid);
        }

        if window_pid == data.pid && unsafe { IsWindowVisible(hwnd) } != 0 {
            data.hwnd = hwnd;
            return 0;
        }

        1
    }

    let mut data = FindWindowData {
        pid,
        hwnd: std::ptr::null_mut(),
    };

    unsafe {
        EnumWindows(Some(enum_proc), &mut data as *mut FindWindowData as LPARAM);
    }

    if data.hwnd.is_null() {
        None
    } else {
        Some(data.hwnd)
    }
}

#[cfg(not(target_os = "windows"))]
fn find_main_window_by_pid(_pid: u32) -> Option<WindowHandle> {
    None
}

#[cfg(target_os = "windows")]
fn activate_window_preserving_state(hwnd: WindowHandle) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };

    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd);
    }
}

#[cfg(not(target_os = "windows"))]
fn activate_window_preserving_state(_hwnd: WindowHandle) {}

#[cfg(target_os = "windows")]
fn minimize_window(hwnd: WindowHandle) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_MINIMIZE, ShowWindow};

    unsafe {
        ShowWindow(hwnd, SW_MINIMIZE);
    }
}

#[cfg(not(target_os = "windows"))]
fn minimize_window(_hwnd: WindowHandle) {}

#[cfg(target_os = "windows")]
fn send_native_space_key() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{KEYEVENTF_KEYUP, VK_SPACE, keybd_event};

    unsafe {
        keybd_event(VK_SPACE as u8, 0, 0, 0);
        keybd_event(VK_SPACE as u8, 0, KEYEVENTF_KEYUP, 0);
    }
}

#[cfg(not(target_os = "windows"))]
fn send_native_space_key() {}

/// 从外部线程发送显示/隐藏事件
pub fn send_overlay_event(state: &SharedOverlayWindowState, event: UserEvent) {
    let proxy = {
        let s = state.lock().unwrap();
        s.proxy.clone()
    };
    if let Some(proxy) = proxy {
        let _ = proxy.send(event);
    } else {
        tracing::warn!(
            "Chrome overlay controller not ready yet, event {:?} dropped",
            event
        );
    }
}
