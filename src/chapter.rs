#[derive(Debug, Clone)]
pub struct Chapter {
    pub line_number: usize,
    pub title: String,
}

/// 使用内置正则表达式检测章节（手动实现，不依赖 regex crate）
/// 正则：^第?\s*(?:\d+|[零一二三四五六七八九十百千万]+)\s*(?:章\s*节?|节|话|話|\s+)
pub fn detect_chapters(lines: &[String]) -> Vec<Chapter> {
    let mut chapters = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        if is_chapter_line(line) {
            chapters.push(Chapter {
                line_number: i,
                title: line.clone(),
            });
        }
    }

    chapters
}

fn is_chapter_line(line: &str) -> bool {
    let s = line.trim();
    if s.is_empty() {
        return false;
    }

    let chars: Vec<char> = s.chars().collect();
    let mut pos = 0;

    // 可选的 "第" 字
    if pos < chars.len() && chars[pos] == '第' {
        pos += 1;
    }

    // 跳过空白
    while pos < chars.len() && chars[pos].is_whitespace() {
        pos += 1;
    }

    // 匹配数字或中文数字
    let num_start = pos;
    if pos < chars.len() && chars[pos].is_ascii_digit() {
        while pos < chars.len() && chars[pos].is_ascii_digit() {
            pos += 1;
        }
    } else {
        // 中文数字
        let cn_digits = "零一二三四五六七八九十百千万";
        while pos < chars.len() && cn_digits.contains(chars[pos]) {
            pos += 1;
        }
    }

    if pos == num_start {
        return false; // 没有匹配到数字
    }

    // 跳过空白
    while pos < chars.len() && chars[pos].is_whitespace() {
        pos += 1;
    }

    // 匹配章节关键词
    if pos >= chars.len() {
        return false;
    }

    let remaining: String = chars[pos..].iter().collect();

    // 匹配 "章" 或 "章节" 或 "节"
    if remaining.starts_with("章节") || remaining.starts_with("章") {
        return true;
    }
    if remaining.starts_with("节") {
        return true;
    }
    if remaining.starts_with("话") || remaining.starts_with("話") {
        return true;
    }
    // 匹配空白（只有空格的情况）
    if remaining.starts_with(|c: char| c.is_whitespace()) {
        return true;
    }

    false
}