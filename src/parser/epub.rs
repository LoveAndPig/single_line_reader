use super::{ImageInfo, ParseResult};
use std::path::Path;

pub fn parse_epub(path: &Path) -> Option<ParseResult> {
    let mut doc = epub::doc::EpubDoc::new(path).ok()?;

    let spine = doc.spine.clone();
    let mut text_lines = Vec::new();
    let mut image_infos = Vec::new();

    for spine_item in &spine {
        // 使用 idref 在 resources 中查找资源
        let resource = doc.resources.get(&spine_item.idref);
        let path_in_epub = match resource {
            Some(r) => r.path.clone(),
            None => continue,
        };

        let path_str = path_in_epub.to_string_lossy().to_string();
        // get_resource_str_by_path 返回 Option<String>
        if let Some(content) = doc.get_resource_str_by_path(&path_str) {
            extract_text_from_html(&content, &mut text_lines);
        }
    }

    // 提取图片资源 - 先收集路径再处理，避免借用冲突
    let image_resources: Vec<(String, String)> = doc
        .resources
        .iter()
        .filter(|(_, r)| r.mime.starts_with("image/"))
        .map(|(_, r)| (r.path.to_string_lossy().to_string(), r.mime.clone()))
        .collect();

    for (path_str, mime) in image_resources {
        if let Some(data) = doc.get_resource_by_path(&path_str) {
            let format = match mime.as_str() {
                "image/png" => "png",
                "image/jpeg" => "jpg",
                "image/gif" => "gif",
                "image/webp" => "webp",
                _ => "png",
            };
            let marker = format!("[IMAGE:{}]", path_str);
            let line_idx = text_lines.len();
            text_lines.push(marker);
            image_infos.push(ImageInfo {
                line_index: line_idx,
                data,
                format: format.to_string(),
            });
        }
    }

    if text_lines.is_empty() {
        return None;
    }

    Some(ParseResult {
        lines: text_lines,
        images: image_infos,
    })
}

fn extract_text_from_html(html: &str, lines: &mut Vec<String>) {
    let mut in_tag = false;
    let mut in_style_or_script = false;
    let mut current_text = String::new();
    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if c == '<' {
            let remaining: String = chars[i..].iter().take(20).map(|c| c.to_ascii_lowercase()).collect();
            if remaining.starts_with("</style") || remaining.starts_with("</script") {
                in_style_or_script = false;
                while i < chars.len() && chars[i] != '>' {
                    i += 1;
                }
                i += 1;
                continue;
            } else if remaining.starts_with("<style") || remaining.starts_with("<script") {
                in_style_or_script = true;
                while i < chars.len() && chars[i] != '>' {
                    i += 1;
                }
                i += 1;
                continue;
            }

            if !in_style_or_script {
                let tag_lower: String = remaining
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '/')
                    .collect::<String>()
                    .to_ascii_lowercase();
                if matches!(
                    tag_lower.as_str(),
                    "br" | "/p" | "/div" | "/h1" | "/h2" | "/h3" | "/h4" | "/h5" | "/h6" | "/li" | "/tr"
                ) {
                    flush_text(&mut current_text, lines);
                } else if matches!(
                    tag_lower.as_str(),
                    "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "tr"
                ) && !current_text.is_empty()
                {
                    flush_text(&mut current_text, lines);
                }
            }

            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag && !in_style_or_script {
            current_text.push(c);
        }

        i += 1;
    }

    flush_text(&mut current_text, lines);
}

fn flush_text(current: &mut String, lines: &mut Vec<String>) {
    let text = current
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'");
    let trimmed = text.trim().to_string();
    if !trimmed.is_empty() {
        lines.push(trimmed);
    }
    current.clear();
}