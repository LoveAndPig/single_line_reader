/// Screen color picker using Windows API.
/// When activated, hides the app window, captures cursor position on next click,
/// reads the pixel color from screen DC, and returns the RGB values.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{GetDC, GetPixel, ReleaseDC};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_LBUTTON};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, ShowWindow, SW_HIDE, SW_SHOW,
};

/// Start screen color picking in a background thread.
/// main_hwnd is the raw HWND value as isize (thread-safe).
/// Returns the picked color as [r, g, b] in 0..=255 range, or None if cancelled.
pub fn pick_screen_color(main_hwnd: isize) -> Option<[u8; 3]> {
    let (tx, rx) = mpsc::channel();

    // Hide the main window (must be done on the calling thread since HWND isn't Send)
    unsafe {
        let hwnd = windows::Win32::Foundation::HWND(main_hwnd as *mut _);
        let _ = ShowWindow(hwnd, SW_HIDE);
    }

    thread::spawn(move || {
        let result = pick_color_loop();

        // Restore the main window
        unsafe {
            let hwnd = windows::Win32::Foundation::HWND(main_hwnd as *mut _);
            let _ = ShowWindow(hwnd, SW_SHOW);
        }

        let _ = tx.send(result);
    });

    rx.recv().ok().flatten()
}

fn pick_color_loop() -> Option<[u8; 3]> {
    // Small delay to let the window hide before we start polling
    thread::sleep(Duration::from_millis(200));

    let mut last_left_state = false;

    loop {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }

        // Check left mouse button state
        let left_pressed =
            (unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } & 0x8000u16 as i16) != 0;

        // Check escape key to cancel
        let esc_pressed =
            (unsafe { GetAsyncKeyState(VK_ESCAPE.0 as i32) } & 0x8000u16 as i16) != 0;
        if esc_pressed {
            return None;
        }

        // Detect click: button was not pressed before but is now pressed
        if left_pressed && !last_left_state {
            // Capture pixel at cursor position
            let color = capture_pixel(pt.x, pt.y);
            return Some(color);
        }

        last_left_state = left_pressed;
        thread::sleep(Duration::from_millis(16)); // ~60 FPS polling
    }
}

fn capture_pixel(x: i32, y: i32) -> [u8; 3] {
    unsafe {
        let hdc = GetDC(None);
        if hdc.is_invalid() {
            return [128, 128, 128];
        }
        let pixel = GetPixel(hdc, x, y);
        let _ = ReleaseDC(None, hdc);

        let pixel_val = pixel.0;
        let r = (pixel_val & 0xFF) as u8;
        let g = ((pixel_val >> 8) & 0xFF) as u8;
        let b = ((pixel_val >> 16) & 0xFF) as u8;
        [r, g, b]
    }
}

/// Get the raw HWND of the egui viewport (for passing to the picker).
/// Returns 0 if not found.
pub fn find_viewport_hwnd() -> isize {
    let title_wide: Vec<u16> = "单行阅读器"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let hwnd = windows::Win32::UI::WindowsAndMessaging::FindWindowW(
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR::from_raw(title_wide.as_ptr()),
        );
        hwnd.map(|h| h.0 as isize).unwrap_or(0)
    }
}
