use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

const CONFIG_FILE: &str = "config.json";

/// 全局单例
static INSTANCE: std::sync::OnceLock<Mutex<AppConfig>> = std::sync::OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleConfig {
    #[serde(rename = "bg_color")]
    pub bg_color: String,
    #[serde(rename = "font_color")]
    pub font_color: String,
    pub font: String,
    #[serde(rename = "font_size")]
    pub font_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutConfig {
    pub prev_line: String,
    pub next_line: String,
    pub scroll_left: String,
    pub scroll_right: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub always_on_top: bool,
    pub style: StyleConfig,
    pub shortcuts: ShortcutConfig,
    #[serde(rename = "window_width")]
    pub window_width: i32,
    #[serde(rename = "window_height")]
    pub window_height: i32,
    #[serde(rename = "window_x")]
    pub window_x: i32,
    #[serde(rename = "window_y")]
    pub window_y: i32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            always_on_top: false,
            style: StyleConfig {
                bg_color: "#1E1E1E".to_string(),
                font_color: "#E0E0E0".to_string(),
                font: "Microsoft YaHei".to_string(),
                font_size: 18,
            },
            shortcuts: ShortcutConfig {
                prev_line: "Up".to_string(),
                next_line: "Down".to_string(),
                scroll_left: "Left".to_string(),
                scroll_right: "Right".to_string(),
            },
            window_width: 800,
            window_height: 40,
            window_x: 200,
            window_y: 200,
        }
    }
}

impl AppConfig {
    /// 获取全局单例（首次调用时自动从磁盘加载）
    pub fn global() -> &'static Mutex<Self> {
        INSTANCE.get_or_init(|| Mutex::new(Self::load()))
    }

    pub fn config_path() -> PathBuf {
        std::env::current_exe()
            .unwrap_or_default()
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join(CONFIG_FILE)
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            let config = Self::default();
            let _ = config.save();
            config
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::config_path();
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&path, json)
    }

    pub fn parse_color(hex: &str) -> u32 {
        let hex = hex.trim_start_matches('#');
        u32::from_str_radix(hex, 16).unwrap_or(0xE0E0E0)
    }
}