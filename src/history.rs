use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub file_name: String,
    pub file_path: String,
    pub current_line: usize,
    pub total_lines: usize,
    pub updated_at: String,
}

/// 全局单例
static INSTANCE: std::sync::OnceLock<Mutex<HistoryManager>> = std::sync::OnceLock::new();

pub struct HistoryManager {
    conn: Connection,
}

impl HistoryManager {
    /// 获取全局单例
    pub fn global() -> &'static Mutex<Self> {
        INSTANCE.get_or_init(|| Mutex::new(Self::new()))
    }

    pub fn new() -> Self {
        let db_path = get_history_path();
        let conn = Connection::open(&db_path).expect("Failed to open history database");

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                file_path TEXT PRIMARY KEY,
                file_name TEXT NOT NULL,
                current_line INTEGER NOT NULL DEFAULT 0,
                total_lines INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
            );",
        )
        .expect("Failed to create history table");

        HistoryManager { conn }
    }

    /// 添加或更新历史记录（INSERT OR REPLACE）
    pub fn add_entry(
        &mut self,
        file_name: &str,
        file_path: &str,
        current_line: usize,
        total_lines: usize,
    ) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let _ = self.conn.execute(
            "INSERT OR REPLACE INTO history (file_path, file_name, current_line, total_lines, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![file_path, file_name, current_line as i64, total_lines as i64, now],
        );
    }

    /// 获取所有历史记录（按时间倒序）
    pub fn get_entries(&self) -> Vec<HistoryEntry> {
        let mut stmt = self
            .conn
            .prepare("SELECT file_path, file_name, current_line, total_lines, updated_at FROM history ORDER BY updated_at DESC")
            .expect("Failed to prepare query");

        let entries = stmt
            .query_map([], |row| {
                Ok(HistoryEntry {
                    file_path: row.get(0)?,
                    file_name: row.get(1)?,
                    current_line: row.get::<_, i64>(2)? as usize,
                    total_lines: row.get::<_, i64>(3)? as usize,
                    updated_at: row.get(4)?,
                })
            })
            .expect("Failed to query history");

        entries.filter_map(|e| e.ok()).collect()
    }

    /// 删除指定路径的历史记录
    pub fn delete_entry(&mut self, file_path: &str) {
        let _ = self.conn.execute(
            "DELETE FROM history WHERE file_path = ?1",
            params![file_path],
        );
    }

    /// 根据文件路径查询历史记录
    #[allow(dead_code)]
    pub fn get_entry(&self, file_path: &str) -> Option<HistoryEntry> {
        let mut stmt = self
            .conn
            .prepare("SELECT file_path, file_name, current_line, total_lines, updated_at FROM history WHERE file_path = ?1")
            .expect("Failed to prepare query");

        stmt.query_map(params![file_path], |row| {
            Ok(HistoryEntry {
                file_path: row.get(0)?,
                file_name: row.get(1)?,
                current_line: row.get::<_, i64>(2)? as usize,
                total_lines: row.get::<_, i64>(3)? as usize,
                updated_at: row.get(4)?,
            })
        })
        .ok()
        .and_then(|mut rows| rows.next())
        .and_then(|r| r.ok())
    }
}

fn get_history_path() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    exe_dir.join("history.db")
}