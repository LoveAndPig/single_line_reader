use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

const REGEX_FILE: &str = "regex.lsy";

/// 全局单例
static INSTANCE: std::sync::OnceLock<Mutex<RegexConfig>> = std::sync::OnceLock::new();

#[derive(Debug, Clone)]
pub struct RegexConfig {
    /// 用户自定义的正则表达式列表
    pub patterns: Vec<String>,
}

impl Default for RegexConfig {
    fn default() -> Self {
        Self {
            patterns: Vec::new(),
        }
    }
}

impl RegexConfig {
    /// 获取全局单例（首次调用时自动从磁盘加载）
    pub fn global() -> &'static Mutex<Self> {
        INSTANCE.get_or_init(|| Mutex::new(Self::load()))
    }

    pub fn file_path() -> PathBuf {
        std::env::current_exe()
            .unwrap_or_default()
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join(REGEX_FILE)
    }

    pub fn load() -> Self {
        let path = Self::file_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                let patterns: Vec<String> = content
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                return Self { patterns };
            }
        }
        Self::default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::file_path();
        let content = self.patterns.join("\n");
        fs::write(&path, content)
    }

    /// 添加一个正则表达式并持久化
    pub fn add_pattern(&mut self, pattern: &str) -> std::io::Result<()> {
        let p = pattern.trim().to_string();
        if !p.is_empty() && !self.patterns.contains(&p) {
            self.patterns.push(p);
        }
        self.save()
    }

    /// 删除一个正则表达式并持久化
    pub fn remove_pattern(&mut self, index: usize) -> std::io::Result<()> {
        if index < self.patterns.len() {
            self.patterns.remove(index);
        }
        self.save()
    }
}
