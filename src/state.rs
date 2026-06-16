use crate::cache::Cache;
use crate::chapter::Chapter;
use crate::config::AppConfig;
use crate::history::HistoryManager;
use crate::parser::{self, ImageInfo};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

pub struct AppState {
    pub config: AppConfig,
    pub history_db: HistoryManager,
    pub cache: Cache,

    pub current_file_path: Option<String>,
    pub current_file_name: Option<String>,
    pub lines: Vec<String>,
    pub current_line: usize,
    pub scroll_offset: f32,
    pub chapters: Vec<Chapter>,
    pub images: Vec<ImageInfo>,

    pub is_visible: bool,

    // egui 对话框状态
    pub show_style_dialog: bool,
    pub show_shortcut_dialog: bool,
    pub show_chapter_dialog: bool,
    pub show_image_dialog: bool,
    pub show_history_dialog: bool,
    pub show_context_menu: bool,
    pub menu_position: (f32, f32),

    // 样式编辑临时值
    pub tmp_bg_color: [f32; 3],
    pub tmp_font_color: [f32; 3],
    pub tmp_font_name: String,
    pub tmp_font_size: u32,

    // 快捷键编辑临时值
    pub waiting_key: Option<(usize, String)>,

    // 系统字体列表
    pub fonts: Vec<String>,
}

impl AppState {
    pub fn new() -> Self {
        let config = AppConfig::load();
        let bg = AppConfig::parse_color(&config.style.bg_color);
        let fg = AppConfig::parse_color(&config.style.font_color);

        Self {
            config: config.clone(),
            history_db: HistoryManager::new(),
            cache: Cache::new(),
            current_file_path: None,
            current_file_name: None,
            lines: Vec::new(),
            current_line: 0,
            scroll_offset: 0.0,
            chapters: Vec::new(),
            images: Vec::new(),
            is_visible: true,
            show_style_dialog: false,
            show_shortcut_dialog: false,
            show_chapter_dialog: false,
            show_image_dialog: false,
            show_history_dialog: false,
            show_context_menu: false,
            menu_position: (0.0, 0.0),
            tmp_bg_color: hex_to_rgb(bg),
            tmp_font_color: hex_to_rgb(fg),
            tmp_font_name: config.style.font.clone(),
            tmp_font_size: config.style.font_size,
            waiting_key: None,
            fonts: Vec::new(),
        }
    }

    pub fn load_file(&mut self, path: &PathBuf) -> bool {
        let path_str = path.to_string_lossy().to_string();

        // 如果打开的文件与当前不同，先保存当前阅读进度
        if self.current_file_path.as_deref() != Some(&path_str) {
            self.save_history();
        }

        if let Some(cached_lines) = self.cache.get_content(&path_str) {
            self.lines = cached_lines.clone();
            if let Some(cached_chapters) = self.cache.get_chapters(&path_str) {
                self.chapters = cached_chapters.clone();
            }
        } else {
            match parser::parse_file(path) {
                Some(result) => {
                    self.lines = result.lines;
                    self.images = result.images;
                    let chapters = crate::chapter::detect_chapters(&self.lines);
                    self.cache.put_content(&path_str, self.lines.clone());
                    self.cache.put_chapters(&path_str, chapters.clone());
                    self.chapters = chapters;
                }
                None => return false,
            }
        }

        self.current_file_path = Some(path_str);
        self.current_file_name = Some(parser::file_stem(path));
        self.current_line = 0;
        self.scroll_offset = 0.0;
        true
    }

    pub fn save_history(&mut self) {
        if let (Some(ref name), Some(ref path)) =
            (&self.current_file_name, &self.current_file_path)
        {
            self.history_db
                .add_entry(name, path, self.current_line, self.lines.len());
        }
    }

    pub fn current_line_text(&self) -> String {
        if self.lines.is_empty() {
            return "请右键选择文件开始阅读".to_string();
        }
        if self.current_line >= self.lines.len() {
            return "(已到达末尾)".to_string();
        }
        let line = &self.lines[self.current_line];
        if line.starts_with("[IMAGE:") {
            return "点击显示图片".to_string();
        }
        line.clone()
    }

    pub fn get_current_image(&self) -> Option<&ImageInfo> {
        if self.current_line >= self.lines.len() {
            return None;
        }
        let line = &self.lines[self.current_line];
        if line.starts_with("[IMAGE:") {
            let _src = &line[7..line.len() - 1];
            return self
                .images
                .iter()
                .find(|img| self.lines.get(img.line_index).map(|l| l.as_str()) == Some(line));
        }
        None
    }

    pub fn next_line(&mut self) {
        if self.current_line + 1 < self.lines.len() {
            self.current_line += 1;
            self.scroll_offset = 0.0;
        }
    }

    pub fn prev_line(&mut self) {
        if self.current_line > 0 {
            self.current_line -= 1;
            self.scroll_offset = 0.0;
        }
    }

    pub fn scroll_left(&mut self) {
        self.scroll_offset = (self.scroll_offset - 30.0).max(0.0);
    }

    pub fn scroll_right(&mut self) {
        self.scroll_offset += 30.0;
    }

    pub fn goto_line(&mut self, line: usize) {
        if line < self.lines.len() {
            self.current_line = line;
            self.scroll_offset = 0.0;
        }
    }

    pub fn apply_style(&mut self) {
        self.config.style.bg_color = rgb_to_hex(self.tmp_bg_color);
        self.config.style.font_color = rgb_to_hex(self.tmp_font_color);
        self.config.style.font = self.tmp_font_name.clone();
        self.config.style.font_size = self.tmp_font_size;
        let _ = self.config.save();
    }
}

fn hex_to_rgb(hex: u32) -> [f32; 3] {
    [
        ((hex >> 16) & 0xFF) as f32 / 255.0,
        ((hex >> 8) & 0xFF) as f32 / 255.0,
        (hex & 0xFF) as f32 / 255.0,
    ]
}

fn rgb_to_hex(rgb: [f32; 3]) -> String {
    let r = (rgb[0] * 255.0) as u32;
    let g = (rgb[1] * 255.0) as u32;
    let b = (rgb[2] * 255.0) as u32;
    format!("#{:02X}{:02X}{:02X}", r, g, b)
}

pub type SharedState = Arc<Mutex<AppState>>;

pub static STATE: std::sync::OnceLock<SharedState> = std::sync::OnceLock::new();