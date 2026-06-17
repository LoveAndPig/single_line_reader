use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DispatchMessageW, FindWindowW, GetCursorPos, HICON, IMAGE_ICON, LoadImageW,
    PeekMessageW, PostMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow,
    ShowWindow, TrackPopupMenu, TranslateMessage, CS_DBLCLKS, CW_USEDEFAULT,
    IDC_ARROW, IDI_APPLICATION, IMAGE_CURSOR, LR_DEFAULTSIZE, LR_LOADFROMFILE,
    LR_SHARED, MF_STRING, MSG, PM_REMOVE, SW_SHOW, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
    TPM_RETURNCMD, WM_CLOSE, WM_CREATE, WM_DESTROY, WM_LBUTTONDBLCLK, WM_RBUTTONUP, WNDCLASSW,
    WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::Win32::Graphics::Gdi::{GetStockObject, HBRUSH};

const WM_TRAYICON: u32 = 0x8001;
const ID_TRAY_EXIT: i32 = 1002;

/// 共享标志：主窗口是否可见。托盘线程显示窗口时设为 true，应用隐藏时设为 false
static WINDOW_VISIBLE: std::sync::OnceLock<Arc<AtomicBool>> = std::sync::OnceLock::new();

/// 共享标志：托盘线程是否继续运行
static RUNNING: std::sync::OnceLock<Arc<AtomicBool>> = std::sync::OnceLock::new();

pub fn start_tray_thread(running: Arc<AtomicBool>, window_visible: Arc<AtomicBool>) {
    let _ = WINDOW_VISIBLE.set(window_visible);
    let _ = RUNNING.set(running.clone());
    thread::spawn(move || {
        tray_thread(running);
    });
}

fn encode_utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn find_main_window() -> HWND {
    let title = encode_utf16("单行阅读器");
    unsafe { FindWindowW(PCWSTR::null(), PCWSTR::from_raw(title.as_ptr())).unwrap_or(HWND(std::ptr::null_mut())) }
}

fn show_main_window() {
    // 先设置共享标志，让主线程知道窗口已恢复
    if let Some(flag) = WINDOW_VISIBLE.get() {
        flag.store(true, Ordering::SeqCst);
    }
    let hwnd = find_main_window();
    if !hwnd.0.is_null() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

fn exit_main_app() {
    // 直接通过全局 STATE 保存历史记录，绕过 egui 事件循环
    // （窗口隐藏时 update() 不会被调用，必须在这里直接保存）
    if let Some(state) = crate::state::STATE.get() {
        if let Ok(s) = state.lock() {
            s.save_current_history();
        }
    }

    // 立即停止托盘线程的消息循环
    if let Some(running) = RUNNING.get() {
        running.store(false, Ordering::SeqCst);
    }

    // 发送关闭消息给主窗口（即使窗口隐藏，PostMessage 也会投递到消息队列）
    let hwnd = find_main_window();
    if !hwnd.0.is_null() {
        unsafe {
            let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

fn tray_thread(running: Arc<AtomicBool>) {
    let instance =
        unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap() };
    let hinstance = HINSTANCE(instance.0);

    let class_name = encode_utf16("TrayReaderClass");

    let cursor = unsafe {
        LoadImageW(
            HINSTANCE::default(),
            PCWSTR(IDC_ARROW.0 as *const u16),
            IMAGE_CURSOR,
            0,
            0,
            LR_DEFAULTSIZE | LR_SHARED,
        )
    };
    let hcursor = windows::Win32::UI::WindowsAndMessaging::HCURSOR(cursor.unwrap_or_default().0);

    let wc = WNDCLASSW {
        style: CS_DBLCLKS,
        lpfnWndProc: Some(tray_wndproc),
        hInstance: hinstance,
        hCursor: hcursor,
        hbrBackground: unsafe {
            HBRUSH(GetStockObject(windows::Win32::Graphics::Gdi::BLACK_BRUSH).0)
        },
        lpszClassName: PCWSTR::from_raw(class_name.as_ptr()),
        ..Default::default()
    };
    unsafe {
        RegisterClassW(&wc);
    }

    use windows::core::w;
    let hwnd = match unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            PCWSTR::from_raw(class_name.as_ptr()),
            w!(""),
            WS_POPUP,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1,
            1,
            None,
            None,
            hinstance,
            None,
        )
    } {
        Ok(h) => h,
        Err(_) => {
            eprintln!("[tray] failed to create hidden window");
            return;
        }
    };

    // 加载自定义图标用于托盘，失败时回退到系统默认图标
    let hicon = {
        let exe_dir = std::env::current_exe()
            .unwrap_or_default()
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf();
        let cwd = std::env::current_dir().unwrap_or_default();

        // 尝试多个路径查找图标
        let candidate_paths = vec![
            exe_dir.join("resources").join("reader.ico"),
            cwd.join("resources").join("reader.ico"),
        ];

        let mut loaded = None;
        for icon_path in &candidate_paths {
            let icon_path_str = icon_path.to_string_lossy().to_string();
            let icon_wide = encode_utf16(&icon_path_str);
            match unsafe {
                LoadImageW(
                    HINSTANCE::default(),
                    PCWSTR::from_raw(icon_wide.as_ptr()),
                    IMAGE_ICON,
                    16,
                    16,
                    LR_LOADFROMFILE,
                )
            } {
                Ok(h) if !h.is_invalid() => {
                    eprintln!("[tray] loaded icon from '{}'", icon_path_str);
                    loaded = Some(HICON(h.0));
                    break;
                }
                _ => {
                    eprintln!("[tray] icon not found at '{}'", icon_path_str);
                }
            }
        }

        loaded.unwrap_or_else(|| {
            // 回退到系统默认应用图标
            eprintln!("[tray] falling back to IDI_APPLICATION");
            match unsafe {
                LoadImageW(
                    HINSTANCE::default(),
                    IDI_APPLICATION,
                    IMAGE_ICON,
                    16,
                    16,
                    LR_SHARED,
                )
            } {
                Ok(h) => HICON(h.0),
                Err(e) => {
                    eprintln!("[tray] IDI_APPLICATION also failed: {:?}", e);
                    HICON::default()
                }
            }
        })
    };

    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAYICON,
        hIcon: hicon,
        ..Default::default()
    };

    let tip: Vec<u16> = encode_utf16("单行阅读器");
    let len = tip.len().min(128);
    nid.szTip[..len].copy_from_slice(&tip[..len]);

    unsafe {
        let result = Shell_NotifyIconW(NIM_ADD, &nid);
        if !result.as_bool() {
            eprintln!("[tray] NIM_ADD failed");
        }
        let result = Shell_NotifyIconW(NIM_SETVERSION, &nid);
        if !result.as_bool() {
            eprintln!("[tray] NIM_SETVERSION failed");
        }
    }

    eprintln!("[tray] icon added successfully");

    let mut msg = MSG::default();
    while running.load(Ordering::SeqCst) {
        // 使用 PeekMessageW 非阻塞处理消息，让线程能检查 running 标志
        while unsafe { PeekMessageW(&mut msg, hwnd, 0, 0, PM_REMOVE).as_bool() } {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        thread::sleep(Duration::from_millis(50));
    }

    // 清理托盘图标
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }

    // 发送退出消息给托盘窗口的消息队列，确保窗口过程收到
    unsafe {
        let _ = PostQuitMessage(0);
    }
}

unsafe extern "system" fn tray_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => LRESULT(0),
        WM_TRAYICON => {
            let event = (lparam.0 & 0xFFFF) as u32;

            if event == WM_LBUTTONDBLCLK {
                show_main_window();
            } else if event == WM_RBUTTONUP {
                let menu = unsafe { CreatePopupMenu().unwrap() };
                let exit = encode_utf16("退出");
                unsafe {
                    let _ = AppendMenuW(
                        menu,
                        MF_STRING,
                        ID_TRAY_EXIT as usize,
                        PCWSTR::from_raw(exit.as_ptr()),
                    );
                }

                let mut pt = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut pt);
                };

                unsafe {
                    let _ = SetForegroundWindow(hwnd);
                    let cmd = TrackPopupMenu(
                        menu,
                        TPM_BOTTOMALIGN | TPM_LEFTALIGN | TPM_RETURNCMD,
                        pt.x,
                        pt.y,
                        0,
                        hwnd,
                        None,
                    );
                    let _ = DestroyMenu(menu);

                    if cmd.0 == ID_TRAY_EXIT {
                        exit_main_app();
                    }
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}