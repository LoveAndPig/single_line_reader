use super::{ImageInfo, ParseResult};
use docx_rs::*;
use std::io::Read;
use std::path::Path;

pub fn parse_docx(path: &Path) -> Option<ParseResult> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).ok()?;

    let doc = read_docx(&data).ok()?;

    let mut lines = Vec::new();
    let mut current_text = String::new();
    let mut image_infos = Vec::new();

    // Build a media lookup for image matching within paragraphs
    let media_map: std::collections::HashMap<String, Vec<u8>> =
        doc.media.iter().cloned().collect();

    // 遍历文档的段落和表格，保持文字与图片的顺序
    for item in &doc.document.children {
        match item {
            DocumentChild::Paragraph(p) => {
                extract_paragraph_with_images(p, &mut current_text, &mut lines, &media_map, &mut image_infos);
            }
            DocumentChild::Table(t) => {
                extract_table_text(t, &mut lines);
            }
            _ => {} // 忽略 BookmarkStart, BookmarkEnd 等
        }
    }

    // 刷新最后的文本
    flush_text(&mut current_text, &mut lines);

    // 追加未在正文中引用的图片（fallback）
    let used_paths: std::collections::HashSet<String> = image_infos
        .iter()
        .filter_map(|img| {
            // 从 lines 中反查该 image 对应的 marker
            if img.line_index < lines.len() {
                let marker = &lines[img.line_index];
                if marker.starts_with("[IMAGE:") {
                    Some(marker[7..marker.len() - 1].to_string())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    for (name, img_data) in &doc.media {
        if !used_paths.contains(name.as_str()) {
            let format = detect_image_format(img_data);
            let marker = format!("[IMAGE:{}]", name);
            lines.push(marker);
            image_infos.push(ImageInfo {
                line_index: lines.len() - 1,
                data: img_data.clone(),
                format,
            });
        }
    }

    if lines.is_empty() {
        return None;
    }

    Some(ParseResult {
        lines,
        images: image_infos,
    })
}

fn extract_paragraph_with_images(
    p: &Paragraph,
    current_text: &mut String,
    lines: &mut Vec<String>,
    media: &std::collections::HashMap<String, Vec<u8>>,
    image_infos: &mut Vec<ImageInfo>,
) {
    for child in &p.children {
        match child {
            ParagraphChild::Run(run) => {
                let mut has_image = false;
                for run_child in &run.children {
                    match run_child {
                        RunChild::Text(text) => {
                            current_text.push_str(&text.text);
                        }
                        RunChild::Drawing(_drawing) => {
                            // 图片出现在文本中间：先刷新文本，再插入图片
                            flush_text(current_text, lines);
                            has_image = true;
                            // 尝试从 drawing 中提取图片引用
                            // docx-rs 的 Drawing 包含图片的 rId 引用
                            // 我们无法直接从 Drawing 获取图片数据，
                            // 但可以通过 media 匹配
                        }
                        _ => {}
                    }
                }
                // 如果这个 Run 包含图片，标记后插入占位
                if has_image {
                    // 插入一个占位符，后续通过 media 匹配
                    // 由于 docx-rs 的限制，我们在此处标记占位
                    let idx = lines.len();
                    lines.push("[IMAGE:inline]".to_string());
                    // 尝试查找对应的图片数据
                    if let Some((name, data)) = find_next_unused_media(media, image_infos) {
                        let format = detect_image_format(&data);
                        lines[idx] = format!("[IMAGE:{}]", name);
                        image_infos.push(ImageInfo {
                            line_index: idx,
                            data: data.clone(),
                            format,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    // 每个段落结束换行
    flush_text(current_text, lines);
}

/// 查找下一个未被使用的 media 图片
fn find_next_unused_media(
    media: &std::collections::HashMap<String, Vec<u8>>,
    used: &[ImageInfo],
) -> Option<(String, Vec<u8>)> {
    let used_count = used.len();
    let mut skipped = 0;
    for (name, data) in media {
        if skipped >= used_count {
            return Some((name.clone(), data.clone()));
        }
        skipped += 1;
    }
    None
}

fn extract_table_text(table: &Table, lines: &mut Vec<String>) {
    for row in &table.rows {
        let TableChild::TableRow(table_row) = row;
        for cell_child in &table_row.cells {
            let TableRowChild::TableCell(cell) = cell_child;
            for item in &cell.children {
                if let TableCellContent::Paragraph(p) = item {
                    let mut cell_text = String::new();
                    for child in &p.children {
                        if let ParagraphChild::Run(run) = child {
                            for run_child in &run.children {
                                if let RunChild::Text(text) = run_child {
                                    cell_text.push_str(&text.text);
                                }
                            }
                        }
                    }
                    let trimmed = cell_text.trim().to_string();
                    if !trimmed.is_empty() {
                        lines.push(trimmed);
                    }
                }
            }
        }
    }
}

fn detect_image_format(data: &[u8]) -> String {
    if data.len() < 4 {
        return "png".to_string();
    }
    if data[0..4] == [0x89, 0x50, 0x4E, 0x47] {
        "png".to_string()
    } else if data[0..2] == [0xFF, 0xD8] {
        "jpg".to_string()
    } else if data[0..4] == [0x47, 0x49, 0x46, 0x38] {
        "gif".to_string()
    } else if data[0..4] == [0x52, 0x49, 0x46, 0x46] {
        "webp".to_string()
    } else {
        "png".to_string()
    }
}

fn flush_text(current: &mut String, lines: &mut Vec<String>) {
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        lines.push(trimmed);
    }
    current.clear();
}