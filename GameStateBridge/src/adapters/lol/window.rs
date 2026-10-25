//! 窗口控制：阵亡时切到抖音、复活时切回游戏（仅 Windows）
//!
//! 参考 liuxiang-sudo/lol2video 的 window_ctl.py：Win32 SetForegroundWindow 在后台进程下
//! 会被系统静默拒绝，需用 AttachThreadInput + 模拟 ALT 按键 + SwitchToThisWindow 组合兜底，
//! 并关闭前台锁定超时（SPI_SETFOREGROUNDLOCKTIMEOUT）。

#![cfg(windows)]

use std::ptr;
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{keybd_event, VK_MENU, VK_SPACE, KEYEVENTF_KEYUP};
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use windows_sys::core::BOOL;

const SPI_SETFOREGROUNDLOCKTIMEOUT: u32 = 0x2001;
const SPIF_SENDCHANGE: u32 = 0x0002;

struct EnumCtx {
    keywords: Vec<String>,
    results: Vec<HWND>,
}

/// 关闭 Windows 前台锁定超时，使后台进程也能切前台窗口
pub fn disable_foreground_lock() -> bool {
    unsafe {
        SystemParametersInfoW(SPI_SETFOREGROUNDLOCKTIMEOUT, 0, ptr::null_mut(), SPIF_SENDCHANGE)
            == 1
    }
}

/// 枚举所有可见窗口，返回标题包含任一关键字的窗口句柄
pub fn find_windows_by_keywords(keywords: &[&str]) -> Vec<HWND> {
    let lowered: Vec<String> = keywords.iter().map(|k| k.to_lowercase()).collect();
    let mut ctx = EnumCtx {
        keywords: lowered,
        results: Vec::new(),
    };
    unsafe {
        let ptr = &mut ctx as *mut EnumCtx as LPARAM;
        EnumWindows(Some(enum_proc), ptr);
    }
    ctx.results
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    unsafe {
        if IsWindowVisible(hwnd) == 0 {
            return 1;
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return 1;
        }
        let mut buf = vec![0u16; (len + 1) as usize];
        GetWindowTextW(hwnd, buf.as_mut_ptr(), len + 1);
        let title = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
        let ctx = &mut *(lparam as *mut EnumCtx);
        if ctx.keywords.iter().any(|k| title.contains(k)) {
            ctx.results.push(hwnd);
        }
        1
    }
}

fn window_title(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; (len + 1) as usize];
        GetWindowTextW(hwnd, buf.as_mut_ptr(), len + 1);
        String::from_utf16_lossy(&buf[..len as usize])
    }
}

/// 用多种手段强行把窗口切到前台
pub fn force_foreground(hwnd: HWND) -> bool {
    unsafe {
        let my_tid = GetCurrentThreadId();
        for _ in 0..5 {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            } else {
                ShowWindow(hwnd, SW_SHOW);
            }
            BringWindowToTop(hwnd);

            let fg = GetForegroundWindow();
            let fg_tid = GetWindowThreadProcessId(fg, ptr::null_mut());
            let mut attached = false;
            if fg_tid != 0 && fg_tid != my_tid {
                attached = AttachThreadInput(my_tid, fg_tid, 1) != 0;
            }
            // 模拟一次 ALT 按键，满足前台锁对"用户输入"的要求
            keybd_event(VK_MENU as u8, 0, 0, 0usize);
            keybd_event(VK_MENU as u8, 0, KEYEVENTF_KEYUP, 0usize);
            SetForegroundWindow(hwnd);
            if attached {
                AttachThreadInput(my_tid, fg_tid, 0);
            }
            SwitchToThisWindow(hwnd, 1);
            std::thread::sleep(std::time::Duration::from_millis(150));
            if GetForegroundWindow() == hwnd {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        false
    }
}

/// 向当前前台窗口发送一次空格键（用于控制抖音网页视频的播放/暂停）
fn send_space() {
    unsafe {
        // 等窗口真正拿到焦点后再发，避免落到地址栏
        std::thread::sleep(std::time::Duration::from_millis(80));
        keybd_event(VK_SPACE as u8, 0, 0, 0usize);
        std::thread::sleep(std::time::Duration::from_millis(30));
        keybd_event(VK_SPACE as u8, 0, KEYEVENTF_KEYUP, 0usize);
    }
}

/// 聚焦已有的抖音窗口（不新建）。命中则返回其句柄。
fn focus_existing_douyin() -> Option<HWND> {
    let site_kw = ["douyin", "抖音"];
    let browser_kw: [&str; 6] = ["chrome", "edge", "firefox", "brave", "opera", "vivaldi"];
    for hwnd in find_windows_by_keywords(&browser_kw) {
        if site_kw
            .iter()
            .any(|k| window_title(hwnd).to_lowercase().contains(k))
        {
            if force_foreground(hwnd) {
                return Some(hwnd);
            }
        }
    }
    None
}

/// 打开抖音（或切回已有抖音窗口）并置前，随后发空格键**开始播放**视频。
pub fn open_or_focus_douyin(url: &str) -> bool {
    // 已有抖音窗口则直接切过去并播放
    if focus_existing_douyin().is_some() {
        send_space();
        return true;
    }

    // 否则用默认浏览器打开
    let _ = webbrowser::open(url);
    for _ in 0..15 {
        std::thread::sleep(std::time::Duration::from_millis(300));
        if focus_existing_douyin().is_some() {
            send_space();
            return true;
        }
    }
    false
}

/// 聚焦抖音窗口并发送空格**暂停**视频（用于复活切回游戏前先停掉视频）。
/// 用窗口级空格而非全局媒体键，避免误触发 QQ 音乐等后台播放器。
pub fn pause_douyin_via_space() -> bool {
    focus_existing_douyin().is_some().then(|| send_space()).is_some()
}

/// 把英雄联盟窗口切回前台
pub fn bring_lol_to_front() -> bool {
    let kw: [&str; 2] = ["league of legends", "英雄联盟"];
    if let Some(hwnd) = find_windows_by_keywords(&kw).first().copied() {
        return force_foreground(hwnd);
    }
    false
}
