use std::collections::HashMap;

/// 缓存：保存本次打开应用后被读取过的文件内容与章节列表
pub struct Cache {
    /// 文件路径 -> 文件内容行列表
    pub content_cache: HashMap<String, Vec<String>>,
    /// 文件路径 -> 章节列表
    pub chapter_cache: HashMap<String, Vec<crate::chapter::Chapter>>,
}

impl Cache {
    pub fn new() -> Self {
        Self {
            content_cache: HashMap::new(),
            chapter_cache: HashMap::new(),
        }
    }

    pub fn get_content(&self, path: &str) -> Option<&Vec<String>> {
        self.content_cache.get(path)
    }

    pub fn put_content(&mut self, path: &str, lines: Vec<String>) {
        self.content_cache.insert(path.to_string(), lines);
    }

    pub fn get_chapters(&self, path: &str) -> Option<&Vec<crate::chapter::Chapter>> {
        self.chapter_cache.get(path)
    }

    pub fn put_chapters(&mut self, path: &str, chapters: Vec<crate::chapter::Chapter>) {
        self.chapter_cache.insert(path.to_string(), chapters);
    }

    /// 清除指定路径的缓存（用于刷新文件时强制重新解析）
    pub fn clear_entry(&mut self, path: &str) {
        self.content_cache.remove(path);
        self.chapter_cache.remove(path);
    }
}

impl Default for Cache {
    fn default() -> Self {
        Self::new()
    }
}