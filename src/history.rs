use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub file_name: String,
    pub file_path: String,
    pub current_line: usize,
    pub total_lines: usize,
    pub timestamp: String,
}

pub struct HistoryManager {
    db_path: PathBuf,
    entries: Vec<HistoryEntry>,
    max_entries: usize,
}

impl HistoryManager {
    pub fn new() -> Self {
        let db_path = get_history_path();
        let mut entries = Vec::new();
        if let Ok(data) = fs::read_to_string(&db_path) {
            if let Ok(parsed) = serde_json::from_str::<Vec<HistoryEntry>>(&data) {
                entries = parsed;
            }
        }
        // 按时间倒序排序
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        HistoryManager {
            db_path,
            entries,
            max_entries: 100,
        }
    }

    /// 添加或更新历史记录
    pub fn add_entry(
        &mut self,
        file_name: &str,
        file_path: &str,
        current_line: usize,
        total_lines: usize,
    ) {
        // 移除同名文件的历史记录
        self.entries.retain(|e| e.file_path != file_path);

        self.entries.push(HistoryEntry {
            file_name: file_name.to_string(),
            file_path: file_path.to_string(),
            current_line,
            total_lines,
            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        });

        // 限制最大条数
        if self.entries.len() > self.max_entries {
            self.entries.truncate(self.max_entries);
        }

        // 按时间倒序
        self.entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        self.save();
    }

    /// 获取所有历史记录
    pub fn get_entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.entries) {
            if let Some(parent) = self.db_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&self.db_path, json);
        }
    }
}

fn get_history_path() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    exe_dir.join("history.json")
}