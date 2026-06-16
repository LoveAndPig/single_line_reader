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

    // 遍历文档的段落和表格
    for item in &doc.document.children {
        match item {
            DocumentChild::Paragraph(p) => {
                extract_paragraph_text(p, &mut current_text, &mut lines);
            }
            DocumentChild::Table(t) => {
                extract_table_text(t, &mut lines);
            }
            _ => {} // 忽略 BookmarkStart, BookmarkEnd 等
        }
    }

    // 刷新最后的文本
    flush_text(&mut current_text, &mut lines);

    // 提取图片 - 直接使用 doc.media
    let mut image_infos = Vec::new();
    for (name, data) in &doc.media {
        let format = detect_image_format(data);
        let marker = format!("[IMAGE:{}]", name);
        lines.push(marker);
        image_infos.push(ImageInfo {
            line_index: lines.len() - 1,
            data: data.clone(),
            format,
        });
    }

    if lines.is_empty() {
        return None;
    }

    Some(ParseResult {
        lines,
        images: image_infos,
    })
}

fn extract_paragraph_text(
    p: &Paragraph,
    current_text: &mut String,
    lines: &mut Vec<String>,
) {
    for child in &p.children {
        if let ParagraphChild::Run(run) = child {
            for run_child in &run.children {
                if let RunChild::Text(text) = run_child {
                    current_text.push_str(&text.text);
                }
            }
        }
    }

    // 每个段落结束换行
    flush_text(current_text, lines);
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