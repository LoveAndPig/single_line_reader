use super::{ImageInfo, ParseResult};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// Parse EPUB file using zip + quick-xml (both MIT licensed).
/// Extracts text and images in reading order, preserving their relative positions.
pub fn parse_epub(path: &Path) -> Option<ParseResult> {
    // 1. Open EPUB as ZIP
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    // 2. Parse container.xml → find OPF path
    let container_xml = read_zip_entry_str(&mut archive, "META-INF/container.xml")?;
    let opf_path = parse_container_for_opf(&container_xml)?;

    // 3. Parse OPF → get manifest (images + spine items) and spine order
    let opf_xml = read_zip_entry_str(&mut archive, &opf_path)?;
    let (spine_hrefs, image_manifest) = parse_opf(&opf_xml)?;

    // 4. Build image lookup: href -> (data, format) by reading images from ZIP
    let image_map = load_images(&mut archive, &opf_path, &image_manifest);

    // 5. For each spine item, parse raw XHTML, extract text + images in order
    let mut lines: Vec<String> = Vec::new();
    let mut image_infos: Vec<ImageInfo> = Vec::new();

    for href in &spine_hrefs {
        let full_path = resolve_relative(&opf_path, href);
        if let Some(xhtml) = read_zip_entry_str(&mut archive, &full_path) {
            parse_xhtml_content(&xhtml, &image_map, &mut lines, &mut image_infos);
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

fn detect_format(mime: &str) -> &str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/svg+xml" => "svg",
        _ => "png",
    }
}

fn mime_from_ext(path: &str) -> &str {
    let lower = path.to_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".bmp") {
        "image/bmp"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "image/png"
    }
}

/// Load image data from ZIP using paths from the manifest
fn load_images(
    archive: &mut zip::ZipArchive<std::fs::File>,
    opf_path: &str,
    manifest: &[(String, String)], // (id, href)
) -> HashMap<String, (Vec<u8>, String)> {
    let mut map = HashMap::new();

    for (_id, href) in manifest {
        let full_path = resolve_relative(opf_path, href);
        if let Some(data) = read_zip_entry_bytes(archive, &full_path) {
            let format = detect_format(mime_from_ext(href));
            map.insert(href.clone(), (data, format.to_string()));
        }
    }

    map
}

/// Read a ZIP entry as bytes
fn read_zip_entry_bytes(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Option<Vec<u8>> {
    if let Ok(mut file) = archive.by_name(name) {
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).ok()?;
        return Some(buf);
    }

    // Case-insensitive fallback
    let name_lower = name.to_lowercase();
    let mut found_idx: Option<usize> = None;
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            if entry.name().to_lowercase() == name_lower {
                found_idx = Some(i);
                break;
            }
        }
    }
    if let Some(idx) = found_idx {
        if let Ok(mut file) = archive.by_index(idx) {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).ok()?;
            return Some(buf);
        }
    }
    None
}

/// Read a ZIP entry as UTF-8 string
fn read_zip_entry_str(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Option<String> {
    if let Ok(mut file) = archive.by_name(name) {
        let mut buf = String::new();
        file.read_to_string(&mut buf).ok()?;
        return Some(buf);
    }

    // Case-insensitive fallback
    let name_lower = name.to_lowercase();
    let mut found_idx: Option<usize> = None;
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            if entry.name().to_lowercase() == name_lower {
                found_idx = Some(i);
                break;
            }
        }
    }
    if let Some(idx) = found_idx {
        if let Ok(mut file) = archive.by_index(idx) {
            let mut buf = String::new();
            file.read_to_string(&mut buf).ok()?;
            return Some(buf);
        }
    }
    None
}

/// Parse META-INF/container.xml to find the OPF file path
fn parse_container_for_opf(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                let tag_qname = e.name();
                if tag_qname.as_ref() == b"rootfile" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"full-path" {
                            if let Ok(val) = String::from_utf8(attr.value.to_vec()) {
                                return Some(val);
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    None
}

/// Parse OPF: returns (spine_hrefs, image_manifest_items)
/// image_manifest_items: Vec<(id, href)> for items with image/* media type
fn parse_opf(xml: &str) -> Option<(Vec<String>, Vec<(String, String)>)> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut in_spine = false;
    let mut spine_items: Vec<String> = Vec::new();
    let mut manifest_map: HashMap<String, (String, String)> = HashMap::new(); // id -> (href, media_type)
    let mut image_manifest: Vec<(String, String)> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let tag_bytes = e.name();
                let tag = tag_bytes.as_ref();
                match tag {
                    b"item" if !in_spine => {
                        // Manifest item
                        let mut id = None;
                        let mut href = None;
                        let mut media_type = None;
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"id" => id = String::from_utf8(attr.value.to_vec()).ok(),
                                b"href" => {
                                    href = String::from_utf8(attr.value.to_vec()).ok()
                                }
                                b"media-type" => {
                                    media_type =
                                        String::from_utf8(attr.value.to_vec()).ok()
                                }
                                _ => {}
                            }
                        }
                        if let (Some(id_str), Some(href_str), Some(mime)) =
                            (id, href, media_type)
                        {
                            if mime.starts_with("image/") {
                                image_manifest.push((id_str.clone(), href_str.clone()));
                            }
                            manifest_map
                                .insert(id_str, (href_str, mime));
                        }
                    }
                    b"spine" => {
                        in_spine = true;
                    }
                    b"itemref" if in_spine => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"idref" {
                                if let Ok(idref) =
                                    String::from_utf8(attr.value.to_vec())
                                {
                                    if let Some((href, _)) = manifest_map.get(&idref)
                                    {
                                        spine_items.push(href.clone());
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let tag_qname = e.name();
                if tag_qname.as_ref() == b"spine" {
                    in_spine = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    if spine_items.is_empty() {
        None
    } else {
        Some((spine_items, image_manifest))
    }
}

/// Resolve a relative path against the OPF directory.
/// Always returns forward slashes since ZIP entries use forward slashes.
fn resolve_relative(base: &str, relative: &str) -> String {
    let base_dir = std::path::Path::new(base)
        .parent()
        .unwrap_or(std::path::Path::new(""));

    let mut result = String::new();

    // Use forward slashes - ZIP entries always use forward slashes
    if let Some(parent) = base_dir.to_str() {
        result.push_str(parent);
        if !result.is_empty() && !result.ends_with('/') {
            result.push('/');
        }
    }

    for component in relative.split('/') {
        match component {
            ".." => {
                // Pop last path component
                if let Some(pos) = result[..result.len().saturating_sub(1)].rfind('/') {
                    result.truncate(pos + 1);
                } else {
                    result.clear();
                }
            }
            "." => {}
            _ => {
                result.push_str(component);
                result.push('/');
            }
        }
    }

    // Remove trailing slash
    if result.ends_with('/') && result.len() > 1 {
        result.pop();
    }

    result
}

/// Parse XHTML content, extracting text paragraphs and images in order
fn parse_xhtml_content(
    xhtml: &str,
    image_map: &HashMap<String, (Vec<u8>, String)>,
    lines: &mut Vec<String>,
    image_infos: &mut Vec<ImageInfo>,
) {
    let mut reader = Reader::from_str(xhtml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut current_text = String::new();
    let mut skip_content = false;

    let block_tags: &[&[u8]] = &[
        b"p", b"div", b"h1", b"h2", b"h3", b"h4", b"h5", b"h6",
        b"li", b"td", b"th", b"blockquote", b"pre", b"section",
        b"article", b"header", b"footer", b"nav", b"br", b"hr",
        b"title", b"tr",
    ];

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let tag_bytes = e.name();
                let tag = tag_bytes.as_ref();

                // Skip style/script content
                if tag.eq_ignore_ascii_case(b"style")
                    || tag.eq_ignore_ascii_case(b"script")
                {
                    if matches!(reader.read_event_into(&mut buf), Ok(Event::Empty(_)))
                    {
                        // self-closing
                    } else {
                        skip_content = true;
                    }
                    buf.clear();
                    continue;
                }

                // Image tag
                if tag.eq_ignore_ascii_case(b"img") {
                    flush_text(&mut current_text, lines);

                    let mut src = None;
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"src" {
                            src = String::from_utf8(attr.value.to_vec()).ok();
                            break;
                        }
                    }

                    if let Some(src_path) = src {
                        if let Some((data, format)) =
                            find_image(image_map, &src_path)
                        {
                            let marker = format!("[IMAGE:{}]", src_path);
                            let line_idx = lines.len();
                            lines.push(marker);
                            image_infos.push(ImageInfo {
                                line_index: line_idx,
                                data: data.clone(),
                                format: format.clone(),
                            });
                        }
                    }
                    buf.clear();
                    continue;
                }

                // <br/> tags
                if tag.eq_ignore_ascii_case(b"br") {
                    flush_text(&mut current_text, lines);
                    buf.clear();
                    continue;
                }
            }
            Ok(Event::End(ref e)) => {
                let tag_bytes = e.name();
                let tag = tag_bytes.as_ref();

                if tag.eq_ignore_ascii_case(b"style")
                    || tag.eq_ignore_ascii_case(b"script")
                {
                    skip_content = false;
                    buf.clear();
                    continue;
                }

                if block_tags
                    .iter()
                    .any(|bt| tag.eq_ignore_ascii_case(bt))
                {
                    flush_text(&mut current_text, lines);
                }
            }
            Ok(Event::Text(ref e)) => {
                if !skip_content {
                    if let Ok(text) = e.unescape() {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            if !current_text.is_empty() {
                                current_text.push(' ');
                            }
                            current_text.push_str(trimmed);
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    flush_text(&mut current_text, lines);
}

fn flush_text(current: &mut String, lines: &mut Vec<String>) {
    let text = current.trim().to_string();
    if !text.is_empty() {
        lines.push(text);
    }
    current.clear();
}

/// Find image data by matching src path against image map entries
fn find_image(
    image_map: &HashMap<String, (Vec<u8>, String)>,
    src: &str,
) -> Option<(Vec<u8>, String)> {
    if let Some(result) = image_map.get(src) {
        return Some(result.clone());
    }

    let decoded = url_decode(src);
    if decoded != src {
        if let Some(result) = image_map.get(&decoded) {
            return Some(result.clone());
        }
    }

    let src_filename = std::path::Path::new(src)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(src);

    for (href, data) in image_map {
        let href_filename = std::path::Path::new(href)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(href);
        if href_filename == src_filename
            || href.ends_with(src)
            || src.ends_with(href.as_str())
        {
            return Some(data.clone());
        }
    }

    None
}

fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            } else {
                result.push('%');
                result.push_str(&hex);
            }
        } else {
            result.push(c);
        }
    }
    result
}