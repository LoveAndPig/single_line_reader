#![windows_subsystem = "windows"]

mod app;
mod cache;
mod chapter;
mod config;
mod history;
mod parser;
mod state;
mod tray;

use state::SharedState;
use state::STATE;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

fn main() -> Result<(), eframe::Error> {
    // 初始化应用状态
    let mut app_state = state::AppState::new();

    // 枚举系统字体
    app_state.fonts = enumerate_fonts();

    let state: SharedState = Arc::new(Mutex::new(app_state));

    let running = Arc::new(AtomicBool::new(true));
    let window_visible = Arc::new(AtomicBool::new(true));

    // 启动托盘线程
    tray::start_tray_thread(running.clone(), window_visible.clone());

    // 将状态存储到全局变量
    STATE.set(state.clone()).ok();

    // 读取配置
    let config = {
        let s = state.lock().unwrap();
        s.config.clone()
    };

    // 构建 eframe 选项
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([config.window_width as f32, config.window_height as f32])
        .with_position([config.window_x as f32, config.window_y as f32])
        .with_decorations(false)
        .with_resizable(true)
        .with_title("单行阅读器");

    if config.always_on_top {
        viewport = viewport.with_always_on_top();
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "单行阅读器",
        options,
        Box::new(|cc| {
            // 配置中文字体支持 - 从 Windows 系统字体目录加载中文字体
            let mut fonts = egui::FontDefinitions::default();
            let font_dirs = vec![
                "C:\\Windows\\Fonts",
                "C:\\WINNT\\Fonts",
            ];

            let chinese_font_files = vec![
                ("msyh.ttc", "Microsoft YaHei"),
                ("msyhbd.ttc", "Microsoft YaHei"),
                ("simsun.ttc", "SimSun"),
                ("simhei.ttf", "SimHei"),
            ];

            let mut loaded_chinese_fonts = Vec::new();
            for font_dir_str in &font_dirs {
                let font_dir = std::path::Path::new(font_dir_str);
                if !font_dir.exists() {
                    continue;
                }
                for (file_name, font_name) in &chinese_font_files {
                    let font_path = font_dir.join(file_name);
                    if font_path.exists() {
                        if let Ok(data) = std::fs::read(&font_path) {
                            fonts.font_data.insert(
                                font_name.to_string(),
                                std::sync::Arc::new(egui::FontData::from_owned(data)),
                            );
                            loaded_chinese_fonts.push(font_name.to_string());
                        }
                    }
                }
            }

            // 在字体列表前插入加载成功的中文字体
            for family in [
                egui::FontFamily::Proportional,
                egui::FontFamily::Monospace,
            ] {
                let entry = fonts.families.entry(family).or_insert_with(Vec::new);
                for f in &loaded_chinese_fonts {
                    if !entry.contains(f) {
                        entry.insert(0, f.clone());
                    }
                }
            }
            cc.egui_ctx.set_fonts(fonts);

            Ok(Box::new(app::ReaderApp::new(state, running.clone(), window_visible)))
        }),
    )?;

    // 通知托盘线程退出
    running.store(false, Ordering::SeqCst);
    // 给托盘线程一点时间清理
    std::thread::sleep(std::time::Duration::from_millis(100));

    Ok(())
}

fn enumerate_fonts() -> Vec<String> {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::Graphics::Gdi::{
        EnumFontFamiliesExW, GetDC, LOGFONTW, FONT_CHARSET, ReleaseDC,
    };

    let mut fonts = Vec::new();

    let hdc = unsafe { GetDC(None) };

    let callback: windows::Win32::Graphics::Gdi::FONTENUMPROCW = Some(enum_fonts_proc);

    let lf = LOGFONTW {
        lfCharSet: FONT_CHARSET(1),
        lfFaceName: [0; 32],
        ..Default::default()
    };

    let user_data = &mut fonts as *mut Vec<String> as isize;

    unsafe {
        EnumFontFamiliesExW(hdc, &lf, callback, LPARAM(user_data), 0);
    }

    unsafe { ReleaseDC(None, hdc) };

    fonts.sort();
    fonts.dedup();
    fonts
}

extern "system" fn enum_fonts_proc(
    lplf: *const windows::Win32::Graphics::Gdi::LOGFONTW,
    _lptm: *const windows::Win32::Graphics::Gdi::TEXTMETRICW,
    _fonttype: u32,
    lparam: windows::Win32::Foundation::LPARAM,
) -> i32 {
    if lplf.is_null() {
        return 0;
    }
    let font = unsafe { &*lplf };
    let face_name = String::from_utf16_lossy(&font.lfFaceName);
    let face_name = face_name.trim_matches('\0').to_string();

    if !face_name.is_empty() && !face_name.starts_with('@') {
        let fonts = unsafe { &mut *(lparam.0 as *mut Vec<String>) };
        if !fonts.contains(&face_name) {
            fonts.push(face_name);
        }
    }
    1
}