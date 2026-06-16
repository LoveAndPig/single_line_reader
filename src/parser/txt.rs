use super::ParseResult;
use encoding_rs::*;
use std::fs;
use std::path::Path;

/// 解析 TXT 文件，自动检测编码
pub fn parse_txt(path: &Path) -> Option<ParseResult> {
    let bytes = fs::read(path).ok()?;

    // 跳过 BOM
    let (bytes_no_bom, encoding_hint) = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        (&bytes[3..], Some(UTF_8))
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        (&bytes[2..], Some(UTF_16LE))
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        (&bytes[2..], Some(UTF_16BE))
    } else {
        (bytes.as_slice(), None)
    };

    // 编码检测：先尝试 UTF-8，失败则尝试 GBK
    let encoding = encoding_hint.unwrap_or_else(|| {
        // 尝试 UTF-8 解码
        if std::str::from_utf8(bytes_no_bom).is_ok() {
            UTF_8
        } else {
            // 尝试 GBK (中文编码)
            encoding_rs::Encoding::for_label(b"gbk").unwrap_or(UTF_8)
        }
    });

    let (text, _actual_encoding, had_errors) = encoding.decode(bytes_no_bom);

    if had_errors {
        // 如果检测的编码有错误，尝试 UTF-8
        if let Ok(utf8_text) = std::str::from_utf8(bytes_no_bom) {
            let lines: Vec<String> = utf8_text
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            return Some(ParseResult {
                lines,
                images: Vec::new(),
            });
        }
    }

    let lines: Vec<String> = text
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    Some(ParseResult {
        lines,
        images: Vec::new(),
    })
}