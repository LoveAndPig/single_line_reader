pub mod docx;
pub mod epub;
pub mod txt;

use std::path::Path;

/// 解析结果：纯文本行列表 + 图片资源列表
pub struct ParseResult {
    pub lines: Vec<String>,
    /// 图片资源 (行索引, 图片数据, 图片格式)
    pub images: Vec<ImageInfo>,
}

#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub line_index: usize,
    pub data: Vec<u8>,
    pub format: String, // "png", "jpg", etc.
}

/// 根据文件扩展名选择合适的解析器
pub fn parse_file(path: &Path) -> Option<ParseResult> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "txt" => txt::parse_txt(path),
        "epub" => epub::parse_epub(path),
        "docx" => docx::parse_docx(path),
        "doc" => {
            // DOC 格式是二进制格式，解析非常复杂，暂不支持
            // 尝试作为 DOCX 处理
            eprintln!("警告: .doc 格式暂不支持，请转换为 .docx 格式");
            None
        }
        _ => {
            eprintln!("不支持的文件格式: {}", ext);
            None
        }
    }
}

/// 获取不带扩展名的文件名
pub fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("未知文件")
        .to_string()
}