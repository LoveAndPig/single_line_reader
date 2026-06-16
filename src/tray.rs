use crate::app::TrayCmd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
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
    DispatchMessageW, GetCursorPos, HICON, IMAGE_ICON, LoadImageW,
    PeekMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow, TrackPopupMenu,
    TranslateMessage, CS_DBLCLKS, CW_USEDEFAULT, IDC_ARROW, IDI_APPLICATION,
    IMAGE_CURSOR, LR_DEFAULTSIZE, LR_SHARED, MF_STRING, MSG, PM_REMOVE,
    TPM_BOTTOMALIGN, TPM_LEFTALIGN, WM_CREATE, WM_DESTROY, WM_LBUTTONDBLCLK,
    WM_RBUTTONUP, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::Win32::Graphics::Gdi::{GetStockObject, HBRUSH};

const WM_TRAYICON: u32 = 0x8001;
const ID_TRAY_SHOW: i32 = 1001;
const ID_TRAY_EXIT: i32 = 1002;

pub fn start_tray_thread(tx: Sender<TrayCmd>, running: Arc<AtomicBool>) {
    thread::spawn(move || {
        tray_thread(tx, running);
    });
}

fn encode_utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// 用于存储 sender 指针的包装
struct TrayUserData {
    tx: Sender<TrayCmd>,
}

fn tray_thread(tx: Sender<TrayCmd>, running: Arc<AtomicBool>) {
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

    let user_data = Box::new(TrayUserData {
        tx: tx.clone(),
    });
    let user_data_ptr = Box::into_raw(user_data) as *const std::ffi::c_void;

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
            Some(user_data_ptr),
        )
    } {
        Ok(h) => h,
        Err(_) => {
            let _ = tx.send(TrayCmd::Exit);
            return;
        }
    };

    // 加载系统图标用于托盘 - 使用 LoadImageW 获取 16x16 小图标
    let hicon = match unsafe {
        LoadImageW(
            HINSTANCE::default(),
            IDI_APPLICATION,
            IMAGE_ICON,
            16,  // 小图标宽度 (SM_CXSMICON)
            16,  // 小图标高度 (SM_CYSMICON)
            LR_SHARED,
        )
    } {
        Ok(h) => HICON(h.0),
        Err(e) => {
            eprintln!("[tray] LoadImageW failed: {:?}", e);
            HICON::default()
        }
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

    // 释放 user data
    unsafe {
        let _ = Box::from_raw(user_data_ptr as *mut TrayUserData);
    }
}

unsafe extern "system" fn tray_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let cs =
                &*(lparam.0 as *const windows::Win32::UI::WindowsAndMessaging::CREATESTRUCTW);
            let user_data_ptr = cs.lpCreateParams;
            if !user_data_ptr.is_null() {
                unsafe {
                    windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                        hwnd,
                        windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                        user_data_ptr as isize,
                    );
                }
            }
            LRESULT(0)
        }
        WM_TRAYICON => {
            let event = (lparam.0 & 0xFFFF) as u32;
            let ptr = unsafe {
                windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                    hwnd,
                    windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                )
            };
            let tx = if ptr != 0 {
                let data = &*(ptr as *const TrayUserData);
                Some(data.tx.clone())
            } else {
                None
            };

            if event == WM_LBUTTONDBLCLK {
                if let Some(tx) = tx {
                    let _ = tx.send(TrayCmd::ToggleVisibility);
                }
            } else if event == WM_RBUTTONUP {
                let menu = unsafe { CreatePopupMenu().unwrap() };
                let show_hide = encode_utf16("显示/隐藏");
                let exit = encode_utf16("退出");
                unsafe {
                    let _ = AppendMenuW(
                        menu,
                        MF_STRING,
                        ID_TRAY_SHOW as usize,
                        PCWSTR::from_raw(show_hide.as_ptr()),
                    );
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
                        TPM_BOTTOMALIGN | TPM_LEFTALIGN,
                        pt.x,
                        pt.y,
                        0,
                        hwnd,
                        None,
                    );
                    let _ = DestroyMenu(menu);

                    if cmd.0 == ID_TRAY_SHOW {
                        if let Some(tx) = &tx {
                            let _ = tx.send(TrayCmd::ToggleVisibility);
                        }
                    } else if cmd.0 == ID_TRAY_EXIT {
                        if let Some(tx) = &tx {
                            let _ = tx.send(TrayCmd::Exit);
                        }
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