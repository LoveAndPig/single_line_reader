use super::{ImageInfo, ParseResult};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

pub fn parse_docx(path: &Path) -> Option<ParseResult> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    // 1. 读取 relationships：rId → 媒体文件路径
    let rels = read_rels(&mut archive);

    // 2. 读取 document.xml
    let doc_xml = read_zip_entry_str(&mut archive, "word/document.xml")?;

    // 3. 按文档顺序解析文本和图片引用
    let mut lines: Vec<String> = Vec::new();
    let mut image_refs: Vec<ImageRef> = Vec::new();
    let mut current_text = String::new();
    let mut in_text = false;

    let mut reader = Reader::from_str(&doc_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local = e.local_name();
                let name = std::str::from_utf8(local.as_ref()).unwrap_or("");
                match name {
                    "t" => {
                        in_text = true;
                    }
                    "blip" => {
                        // <a:blip r:embed="rId1"/> — 自闭合，Empty 事件也会进入此分支
                        for attr in e.attributes().flatten() {
                            let attr_local = attr.key.local_name();
                            let key = std::str::from_utf8(attr_local.as_ref()).unwrap_or("");
                            if key == "embed" {
                                let val = std::str::from_utf8(&attr.value)
                                    .unwrap_or("")
                                    .to_string();
                                flush_text(&mut current_text, &mut lines);
                                image_refs.push(ImageRef {
                                    line_index: lines.len(),
                                    r_id: val,
                                });
                                lines.push(String::new());
                            }
                        }
                    }
                    "p" => {
                        current_text.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) if in_text => {
                if let Ok(t) = e.unescape() {
                    current_text.push_str(&t);
                }
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name();
                let name = std::str::from_utf8(local.as_ref()).unwrap_or("");
                if name == "t" {
                    in_text = false;
                }
                if name == "p" {
                    flush_text(&mut current_text, &mut lines);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    flush_text(&mut current_text, &mut lines);

    if lines.is_empty() {
        return None;
    }

    // 4. 根据 rId 映射读取图片数据，填入占位行
    let mut image_data_map: HashMap<String, (Vec<u8>, String)> = HashMap::new();

    for img_ref in &image_refs {
        if let Some(media_path) = rels.get(&img_ref.r_id) {
            if let Some(data) =
                read_zip_entry_bytes(&mut archive, &format!("word/{}", media_path))
            {
                // 验证是否为有效图片
                if image::load_from_memory(&data).is_ok() {
                    let format = detect_image_format(&data);
                    let marker = format!("[IMAGE:{}]", media_path);
                    lines[img_ref.line_index] = marker;
                    image_data_map
                        .insert(media_path.clone(), (data, format));
                }
            }
        }
    }

    // 清理未匹配到图片数据的空占位行
    lines.retain(|l| !l.is_empty());

    // 重新构建 image_infos：扫描 lines 中的 [IMAGE:...] markers
    let mut image_infos: Vec<ImageInfo> = Vec::new();
    let mut used_paths: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("[IMAGE:") {
            let path = &line[7..line.len() - 1];
            if let Some((data, format)) = image_data_map.remove(path) {
                image_infos.push(ImageInfo {
                    line_index: i,
                    data,
                    format: format.clone(),
                });
                used_paths.insert(path.to_string());
            }
        }
    }

    // 5. 追加文档中未被引用且有效的图片（fallback）
    for (_r_id, media_path) in &rels {
        if !used_paths.contains(media_path.as_str()) {
            if let Some(data) =
                read_zip_entry_bytes(&mut archive, &format!("word/{}", media_path))
            {
                if image::load_from_memory(&data).is_ok() {
                    let format = detect_image_format(&data);
                    let marker = format!("[IMAGE:{}]", media_path);
                    lines.push(marker);
                    image_infos.push(ImageInfo {
                        line_index: lines.len() - 1,
                        data,
                        format,
                    });
                }
            }
        }
    }

    Some(ParseResult {
        lines,
        images: image_infos,
    })
}

struct ImageRef {
    line_index: usize,
    r_id: String,
}

/// 解析 word/_rels/document.xml.rels，返回 image rId → media 路径 的映射
fn read_rels(archive: &mut zip::ZipArchive<std::fs::File>) -> HashMap<String, String> {
    let mut map = HashMap::new();

    let xml = match read_zip_entry_str(archive, "word/_rels/document.xml.rels") {
        Some(s) => s,
        None => return map,
    };

    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                let local = e.local_name();
                let name = std::str::from_utf8(local.as_ref()).unwrap_or("");
                if name == "Relationship" {
                    let mut id = String::new();
                    let mut target = String::new();
                    let mut rel_type = String::new();
                    for attr in e.attributes().flatten() {
                        let attr_local = attr.key.local_name();
                        let key = std::str::from_utf8(attr_local.as_ref()).unwrap_or("");
                        match key {
                            "Id" => {
                                id = std::str::from_utf8(&attr.value).unwrap_or("").to_string()
                            }
                            "Target" => {
                                target =
                                    std::str::from_utf8(&attr.value).unwrap_or("").to_string()
                            }
                            "Type" => {
                                rel_type =
                                    std::str::from_utf8(&attr.value).unwrap_or("").to_string()
                            }
                            _ => {}
                        }
                    }
                    // 只保留图片类型的 relationship
                    if !id.is_empty()
                        && !target.is_empty()
                        && rel_type.contains("image")
                    {
                        map.insert(id, target);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    map
}

fn read_zip_entry_str(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Option<String> {
    let mut s = String::new();
    archive.by_name(name).ok()?.read_to_string(&mut s).ok()?;
    Some(s)
}

fn read_zip_entry_bytes(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Option<Vec<u8>> {
    let mut v = Vec::new();
    archive.by_name(name).ok()?.read_to_end(&mut v).ok()?;
    if v.is_empty() {
        None
    } else {
        Some(v)
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