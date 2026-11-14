use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use zip::ZipArchive;

use crate::models::BookMetadata;

#[derive(Debug, Clone, Default)]
pub struct Extracted {
    pub metadata: BookMetadata,
    pub cover: Option<Vec<u8>>,
    pub page_count: i64,
}

#[derive(Debug, Clone, Default)]
pub struct FilenameInfo {
    pub title: String,
    pub author: String,
}

#[derive(Debug, Clone, Default)]
pub struct PdfInfo {
    pub page_count: i64,
    pub title: String,
    pub authors: String,
    pub publisher: String,
}

pub fn extension_of(rel_path: &str) -> String {
    Path::new(rel_path)
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn is_supported(rel_path: &str) -> bool {
    matches!(
        extension_of(rel_path).as_str(),
        "epub" | "azw3" | "azw" | "mobi" | "pdf" | "txt"
    )
}

pub fn basename_of(rel_path: &str) -> String {
    Path::new(rel_path)
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| rel_path.to_string())
}

pub fn extract(format: &str, bytes: &[u8], rel_path: &str) -> Result<Extracted> {
    match format {
        "epub" => parse_epub(bytes),
        "pdf" => Ok(pdf_extracted(parse_pdf(bytes)?, rel_path)),
        "azw3" | "azw" | "mobi" => parse_mobi_bytes(bytes),
        _ => {
            let fallback = parse_filename(rel_path);
            Ok(Extracted {
                metadata: fallback_metadata(&fallback),
                cover: None,
                page_count: 0,
            })
        }
    }
}

pub fn extract_from_path(format: &str, path: &Path, rel_path: &str) -> Result<Extracted> {
    match format {
        "epub" => parse_epub_path(path),
        "pdf" => Ok(pdf_extracted(parse_pdf_file(path)?, rel_path)),
        "azw3" | "azw" | "mobi" => parse_mobi_path(path),
        _ => {
            let fallback = parse_filename(rel_path);
            Ok(Extracted {
                metadata: fallback_metadata(&fallback),
                cover: None,
                page_count: 0,
            })
        }
    }
}

fn pdf_extracted(info: PdfInfo, rel_path: &str) -> Extracted {
    let fallback = parse_filename(rel_path);
    let fallback_title = repair_mojibake(&fallback.title);
    let title = if fallback_title.trim().is_empty() || fallback_title.contains('\u{FFFD}') {
        repair_mojibake(&info.title)
    } else {
        fallback_title
    };
    let authors = if !fallback.author.trim().is_empty() {
        repair_mojibake(&fallback.author)
    } else if is_generic_pdf_author(&info.authors) {
        String::new()
    } else {
        repair_mojibake(&info.authors)
    };
    Extracted {
        metadata: BookMetadata {
            title,
            authors,
            publisher: info.publisher.trim().to_string(),
            ..Default::default()
        },
        cover: None,
        page_count: info.page_count,
    }
}

fn fallback_metadata(info: &FilenameInfo) -> BookMetadata {
    BookMetadata {
        title: info.title.clone(),
        authors: info.author.clone(),
        ..Default::default()
    }
}

fn parse_mobi_path(path: &Path) -> Result<Extracted> {
    let metadata = mobi::MobiMetadata::from_path(path)
        .with_context(|| format!("无法解析 MOBI/AZW3 元数据: {}", path.display()))?;
    Ok(mobi_extracted(&metadata))
}

fn parse_mobi_bytes(bytes: &[u8]) -> Result<Extracted> {
    let mut cursor = Cursor::new(bytes);
    let metadata =
        mobi::MobiMetadata::from_read(&mut cursor).context("无法解析 MOBI/AZW3 元数据")?;
    Ok(mobi_extracted(&metadata))
}

fn mobi_extracted(metadata: &mobi::MobiMetadata) -> Extracted {
    let title = repair_mojibake(&metadata.title()).trim().to_string();
    let authors = metadata
        .author()
        .map(|value| clean_author(&repair_mojibake(&value)))
        .unwrap_or_default();
    let publisher = metadata
        .publisher()
        .map(|value| repair_mojibake(&value).trim().to_string())
        .unwrap_or_default();
    let description = metadata
        .description()
        .map(|value| repair_mojibake(&value).trim().to_string())
        .unwrap_or_default();
    let isbn = metadata
        .isbn()
        .and_then(|value| {
            extract_isbn(&value).or_else(|| {
                let trimmed = value.trim().to_string();
                (!trimmed.is_empty()).then_some(trimmed)
            })
        })
        .unwrap_or_default();
    let pub_date = metadata
        .publish_date()
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    Extracted {
        metadata: BookMetadata {
            title,
            authors,
            publisher,
            isbn,
            description,
            language: mobi_language(metadata.language()),
            pub_date,
        },
        cover: None,
        page_count: 0,
    }
}

fn mobi_language(language: mobi::headers::Language) -> String {
    use mobi::headers::Language;
    match language {
        Language::Chinese => "zh",
        Language::English => "en",
        Language::Japanese => "ja",
        Language::Korean => "ko",
        Language::French => "fr",
        Language::German => "de",
        Language::Spanish => "es",
        Language::Russian => "ru",
        _ => "",
    }
    .to_string()
}

fn repair_mojibake(value: &str) -> String {
    if value.chars().any(|character| character as u32 > 0xFF) {
        return value.to_string();
    }
    let bytes: Vec<u8> = value.chars().map(|character| character as u8).collect();
    if let Ok(decoded) = std::str::from_utf8(&bytes) {
        if decoded
            .chars()
            .any(|character| matches!(character as u32, 0x3400..=0x9FFF | 0xF900..=0xFAFF))
        {
            return decoded.to_string();
        }
    }
    value.to_string()
}

pub fn parse_epub(bytes: &[u8]) -> Result<Extracted> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).context("无法打开 EPUB zip")?;
    parse_epub_archive(&mut archive)
}

pub fn parse_epub_path(path: &Path) -> Result<Extracted> {
    let file = File::open(path).with_context(|| format!("无法打开 EPUB: {}", path.display()))?;
    let mut archive = ZipArchive::new(file).context("无法打开 EPUB zip")?;
    parse_epub_archive(&mut archive)
}

fn parse_epub_archive<R: Read + std::io::Seek>(archive: &mut ZipArchive<R>) -> Result<Extracted> {
    let container = read_zip_entry(archive, "META-INF/container.xml")
        .context("EPUB 缺少 META-INF/container.xml")?;
    let opf_path = parse_container(&container)?;
    let opf =
        read_zip_entry(archive, &opf_path).with_context(|| format!("EPUB 缺少 OPF: {opf_path}"))?;
    let parsed = parse_opf(&opf)?;

    let mut cover = None;
    let base = Path::new(&opf_path)
        .parent()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let candidates = parsed
        .cover_href
        .clone()
        .or_else(|| parsed.first_image.clone());
    if let Some(href) = candidates {
        let cover_path = resolve_zip_path(&base, &href);
        if let Ok(data) = read_zip_entry(archive, &cover_path) {
            cover = Some(data);
        }
    }

    Ok(Extracted {
        metadata: parsed.metadata,
        cover,
        page_count: 0,
    })
}

fn read_zip_entry<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>> {
    let mut file = archive
        .by_name(name)
        .with_context(|| format!("zip 中找不到 {name}"))?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    Ok(data)
}

fn decode_xml(xml: &[u8]) -> String {
    if let Some((encoding, bom_len)) = encoding_rs::Encoding::for_bom(xml) {
        let (text, _, _) = encoding.decode(&xml[bom_len..]);
        return text.into_owned();
    }
    if let Some(label) = sniff_xml_encoding(xml) {
        if let Some(encoding) = encoding_rs::Encoding::for_label(label.as_bytes()) {
            let (text, _, _) = encoding.decode(xml);
            return text.into_owned();
        }
    }
    if let Ok(text) = std::str::from_utf8(xml) {
        return text.to_string();
    }
    let (text, _, had_errors) = encoding_rs::GB18030.decode(xml);
    if !had_errors {
        return text.into_owned();
    }
    let (text, _, _) = encoding_rs::BIG5.decode(xml);
    text.into_owned()
}

fn sniff_xml_encoding(xml: &[u8]) -> Option<String> {
    let head = &xml[..xml.len().min(256)];
    let text = String::from_utf8_lossy(head);
    let lower = text.to_ascii_lowercase();
    let index = lower.find("encoding")?;
    let rest = text[index + "encoding".len()..].trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &rest[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

fn parse_container(xml: &[u8]) -> Result<String> {
    let text = decode_xml(xml);
    let mut reader = Reader::from_str(&text);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                let name = String::from_utf8_lossy(event.local_name().as_ref()).to_string();
                if name == "rootfile" {
                    if let Some(path) = attribute(&event, b"full-path") {
                        return Ok(path);
                    }
                }
            }
            Ok(Event::Empty(event)) => {
                let name = String::from_utf8_lossy(event.local_name().as_ref()).to_string();
                if name == "rootfile" {
                    if let Some(path) = attribute(&event, b"full-path") {
                        return Ok(path);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(error).context("解析 container.xml 失败"),
            _ => {}
        }
    }
    anyhow::bail!("container.xml 中没有 rootfile")
}

#[derive(Default)]
struct OpfParsed {
    metadata: BookMetadata,
    cover_href: Option<String>,
    first_image: Option<String>,
}

fn parse_opf(xml: &[u8]) -> Result<OpfParsed> {
    let text = decode_xml(xml);
    let mut reader = Reader::from_str(&text);
    reader.config_mut().trim_text(true);

    let mut result = OpfParsed::default();
    let mut creators = Vec::new();
    let mut identifiers = Vec::new();
    let mut current_tag = String::new();
    let mut current_text = String::new();
    let mut cover_id: Option<String> = None;
    let mut manifest: HashMap<String, ManifestItem> = HashMap::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                let name = String::from_utf8_lossy(event.local_name().as_ref()).to_string();
                current_tag = name.clone();
                current_text.clear();
                if name == "meta" {
                    if attribute(&event, b"name").as_deref() == Some("cover") {
                        cover_id = attribute(&event, b"content");
                    }
                } else if name == "item" {
                    if let Some(id) = attribute(&event, b"id") {
                        let href = attribute(&event, b"href").unwrap_or_default();
                        let media_type = attribute(&event, b"media-type").unwrap_or_default();
                        let properties = attribute(&event, b"properties").unwrap_or_default();
                        manifest.insert(
                            id.clone(),
                            ManifestItem {
                                href: href.clone(),
                                properties: properties.clone(),
                            },
                        );
                        if properties
                            .split_whitespace()
                            .any(|value| value == "cover-image")
                        {
                            result.cover_href = Some(href.clone());
                        }
                        if result.first_image.is_none() && media_type.starts_with("image/") {
                            result.first_image = Some(href);
                        }
                    }
                }
            }
            Ok(Event::Empty(event)) => {
                let name = String::from_utf8_lossy(event.local_name().as_ref()).to_string();
                if name == "meta" {
                    if attribute(&event, b"name").as_deref() == Some("cover") {
                        cover_id = attribute(&event, b"content");
                    }
                } else if name == "item" {
                    if let Some(id) = attribute(&event, b"id") {
                        let href = attribute(&event, b"href").unwrap_or_default();
                        let media_type = attribute(&event, b"media-type").unwrap_or_default();
                        let properties = attribute(&event, b"properties").unwrap_or_default();
                        manifest.insert(
                            id.clone(),
                            ManifestItem {
                                href: href.clone(),
                                properties: properties.clone(),
                            },
                        );
                        if properties
                            .split_whitespace()
                            .any(|value| value == "cover-image")
                        {
                            result.cover_href = Some(href.clone());
                        }
                        if result.first_image.is_none() && media_type.starts_with("image/") {
                            result.first_image = Some(href);
                        }
                    }
                }
            }
            Ok(Event::Text(event)) => {
                if matches!(
                    current_tag.as_str(),
                    "title"
                        | "creator"
                        | "publisher"
                        | "identifier"
                        | "description"
                        | "language"
                        | "date"
                ) {
                    current_text.push_str(&event.unescape().unwrap_or_default());
                }
            }
            Ok(Event::End(event)) => {
                let name = String::from_utf8_lossy(event.local_name().as_ref()).to_string();
                let value = current_text.trim().to_string();
                match name.as_str() {
                    "title" if result.metadata.title.is_empty() => result.metadata.title = value,
                    "creator" if !value.is_empty() => creators.push(value),
                    "publisher" if result.metadata.publisher.is_empty() => {
                        result.metadata.publisher = value
                    }
                    "identifier" if !value.is_empty() => identifiers.push(value),
                    "description" if result.metadata.description.is_empty() => {
                        result.metadata.description = value
                    }
                    "language" if result.metadata.language.is_empty() => {
                        result.metadata.language = value
                    }
                    "date" if result.metadata.pub_date.is_empty() => {
                        result.metadata.pub_date = value
                    }
                    _ => {}
                }
                current_tag.clear();
                current_text.clear();
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(error).context("解析 OPF 失败"),
            _ => {}
        }
    }

    result.metadata.authors = creators.join("; ");
    result.metadata.isbn = identifiers
        .iter()
        .find_map(|value| extract_isbn(value))
        .unwrap_or_default();
    if let Some(id) = cover_id {
        if let Some(item) = manifest.get(&id) {
            result.cover_href = Some(item.href.clone());
        }
    }
    if result.cover_href.is_none() {
        if let Some(item) = manifest.values().find(|item| {
            item.properties
                .split_whitespace()
                .any(|value| value == "cover-image")
        }) {
            result.cover_href = Some(item.href.clone());
        }
    }
    Ok(result)
}

#[derive(Debug, Clone)]
struct ManifestItem {
    href: String,
    properties: String,
}

fn attribute(event: &BytesStart<'_>, key: &[u8]) -> Option<String> {
    event
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.as_ref() == key)
        .and_then(|attribute| attribute.unescape_value().ok())
        .map(|value| value.to_string())
}

fn extract_isbn(value: &str) -> Option<String> {
    let digits: String = value
        .chars()
        .filter(|character| character.is_ascii_digit() || matches!(character, 'x' | 'X'))
        .collect();
    if digits.len() == 13 || digits.len() == 10 {
        Some(digits)
    } else {
        None
    }
}

fn resolve_zip_path(base: &str, href: &str) -> String {
    let mut parts: Vec<String> = base
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .map(str::to_string)
        .collect();
    for part in href.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other.to_string()),
        }
    }
    parts.join("/")
}

fn is_generic_pdf_author(value: &str) -> bool {
    let lower = value.to_lowercase();
    lower.is_empty()
        || lower == "home"
        || lower.contains("china-pub")
        || lower.contains("http")
        || lower.contains("www.")
}

pub fn parse_pdf(bytes: &[u8]) -> Result<PdfInfo> {
    let document = lopdf::Document::load_mem(bytes).context("无法解析 PDF")?;
    let page_count = document.get_pages().len() as i64;
    let (title, authors, publisher) = pdf_metadata(&document);
    Ok(PdfInfo {
        page_count,
        title,
        authors,
        publisher,
    })
}

pub fn parse_pdf_file(path: &Path) -> Result<PdfInfo> {
    if let Ok(output) = Command::new("pdfinfo").arg(path).output() {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            let mut info = PdfInfo::default();
            for line in text.lines() {
                if let Some(value) = line.strip_prefix("Pages:") {
                    info.page_count = value.trim().parse().unwrap_or(0);
                } else if let Some(value) = line.strip_prefix("Title:") {
                    info.title = value.trim().to_string();
                } else if let Some(value) = line.strip_prefix("Author:") {
                    info.authors = value.trim().to_string();
                }
            }
            if info.page_count > 0 || !info.title.is_empty() || !info.authors.is_empty() {
                return Ok(info);
            }
        }
    }
    let document = lopdf::Document::load(path).context("无法解析 PDF")?;
    let page_count = document.get_pages().len() as i64;
    let (title, authors, publisher) = pdf_metadata(&document);
    Ok(PdfInfo {
        page_count,
        title,
        authors,
        publisher,
    })
}

fn pdf_metadata(document: &lopdf::Document) -> (String, String, String) {
    let info = document
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|object| match object {
            lopdf::Object::Reference(id) => document
                .get_object(*id)
                .ok()
                .and_then(|object| object.as_dict().ok()),
            lopdf::Object::Dictionary(dictionary) => Some(dictionary),
            _ => None,
        });
    let Some(dictionary) = info else {
        return (String::new(), String::new(), String::new());
    };
    let value = |key: &[u8]| {
        dictionary
            .get(key)
            .ok()
            .and_then(|object| object.as_str().ok())
            .map(|bytes| String::from_utf8_lossy(bytes).to_string())
            .unwrap_or_default()
    };
    (
        value(b"Title").trim().to_string(),
        value(b"Author").trim().to_string(),
        value(b"Publisher").trim().to_string(),
    )
}

pub fn parse_filename(rel_path: &str) -> FilenameInfo {
    let mut stem = repair_mojibake(&basename_of(rel_path));
    loop {
        let trimmed = stem.trim();
        let (open, close) = match (trimmed.rfind('('), trimmed.rfind('（')) {
            (Some(a), Some(b)) if a > b => ('(', Some(a)),
            (None, Some(b)) => ('（', Some(b)),
            (Some(a), None) => ('(', Some(a)),
            (Some(a), Some(b)) if b > a => ('（', Some(b)),
            _ => ('(', None),
        };
        let Some(index) = close else { break };
        if !trimmed.ends_with(')') && !trimmed.ends_with('）') {
            break;
        }
        let close_len = if trimmed.ends_with('）') {
            '）'.len_utf8()
        } else {
            ')'.len_utf8()
        };
        let content = &trimmed[index + open.len_utf8()..trimmed.len() - close_len];
        let lowered = content.to_lowercase();
        if lowered.contains("z-library")
            || lowered.contains("z-lib")
            || lowered.contains("zlibrary")
        {
            stem = trimmed[..index].trim_end().to_string();
            continue;
        }
        break;
    }

    let stem = stem
        .trim()
        .trim_end_matches(['.', '。', '：', ':', '-', '_', ' '])
        .to_string();
    if stem.is_empty() {
        return FilenameInfo {
            title: repair_mojibake(&basename_of(rel_path)),
            author: String::new(),
        };
    }

    if let Some((title, author)) = split_author(&stem) {
        return FilenameInfo {
            title: repair_mojibake(&title),
            author: repair_mojibake(&author),
        };
    }
    FilenameInfo {
        title: stem,
        author: String::new(),
    }
}

fn split_author(stem: &str) -> Option<(String, String)> {
    let (index, open) = matching_open(stem)?;
    if !stem.ends_with(')') && !stem.ends_with('）') {
        return None;
    }
    let close_len = if stem.ends_with('）') {
        '）'.len_utf8()
    } else {
        ')'.len_utf8()
    };
    let content = &stem[index + open.len_utf8()..stem.len() - close_len];
    if is_series_or_edition(content) {
        return None;
    }
    let author = clean_author(content);
    if author.is_empty() || author.chars().count() > 40 {
        return None;
    }
    let title = stem[..index]
        .trim()
        .trim_end_matches(['.', '。', '：', ':', '-', '_', ' '])
        .to_string();
    if title.is_empty() {
        return None;
    }
    Some((title, author))
}

fn matching_open(stem: &str) -> Option<(usize, char)> {
    let mut depth = 0i32;
    for (index, character) in stem.char_indices().rev() {
        match character {
            ')' | '）' => depth += 1,
            '(' | '（' => {
                depth -= 1;
                if depth == 0 {
                    return Some((index, character));
                }
            }
            _ => {}
        }
    }
    None
}

fn is_series_or_edition(content: &str) -> bool {
    const BLACKLIST: &[&str] = &[
        "丛书",
        "系列",
        "全集",
        "文集",
        "作品集",
        "选集",
        "套装",
        "合集",
        "第",
        "卷",
        "册",
        "版",
        "出版社",
        "出品",
        "书系",
        "译丛",
        "名著",
        "经典",
        "读本",
        "教程",
        "教材",
        "指南",
        "手册",
        "辞典",
        "词典",
        "百科",
        "图鉴",
        "绘本",
        "漫画",
        "上下",
        "修订",
        "增订",
        "插图",
        "彩图",
        "英文版",
        "中文版",
        "影印",
        "全译本",
        "套装共",
    ];
    BLACKLIST.iter().any(|word| content.contains(word))
}

fn clean_author(content: &str) -> String {
    let mut value = content.trim().to_string();
    loop {
        let before = value.clone();
        let trimmed = value.trim_start();
        let marker = [('[', ']'), ('【', '】'), ('（', '）'), ('(', ')')]
            .iter()
            .find_map(|(open, close)| {
                trimmed
                    .strip_prefix(*open)
                    .and_then(|rest| rest.find(*close).map(|end| (rest, end)))
            });
        if let Some((rest, end)) = marker {
            let inside = &rest[..end];
            if inside.chars().count() <= 8 && !inside.contains(['，', ',', ';', '；']) {
                value = rest[end..]
                    .trim_start_matches(|character: char| {
                        matches!(character, ']' | '】' | '）' | ')')
                    })
                    .trim()
                    .to_string();
            }
        }
        for suffix in [
            "著", "编著", "主编", "编", "译", "等", "著.", "著。", "编著.", "主编.", "译.",
        ] {
            if let Some(rest) = value.strip_suffix(suffix) {
                value = rest
                    .trim_end_matches([',', '，', '、', ' ', ';', '；'])
                    .to_string();
            }
        }
        value = value
            .trim()
            .trim_matches(['[', ']', '【', '】'])
            .to_string();
        if value == before {
            break;
        }
    }
    value
}

pub fn normalize_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(|character| character.to_lowercase())
        .collect()
}

pub fn primary_author(authors: &str) -> String {
    let first = authors
        .split([';', '；', ',', '，', '、', '/'])
        .find(|part| !part.trim().is_empty())
        .unwrap_or("");
    clean_author(first)
}

pub fn work_key(title: &str, authors: &str) -> String {
    format!(
        "{}|{}",
        normalize_key(title),
        normalize_key(&primary_author(authors))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn make_epub(title: &str, creator: &str, publisher: &str, isbn: &str) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        zip.start_file("META-INF/container.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/content.opf", options).unwrap();
        let opf = format!(
            r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>{title}</dc:title>
    <dc:creator>{creator}</dc:creator>
    <dc:publisher>{publisher}</dc:publisher>
    <dc:identifier>urn:isbn:{isbn}</dc:identifier>
    <dc:description>简介</dc:description>
    <dc:language>zh</dc:language>
    <dc:date>2024-01-01</dc:date>
    <meta name="cover" content="cover-img"/>
  </metadata>
  <manifest>
    <item id="cover-img" href="images/cover.jpg" media-type="image/jpeg"/>
    <item id="text" href="text.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="text"/></spine>
</package>"#
        );
        zip.write_all(opf.as_bytes()).unwrap();
        zip.start_file("OEBPS/images/cover.jpg", options).unwrap();
        zip.write_all(&[0xff, 0xd8, 0xff, 0xd9]).unwrap();
        zip.finish().unwrap().into_inner()
    }

    fn make_pdf() -> Vec<u8> {
        use lopdf::{dictionary, Document, Object};
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        let info_id = document.add_object(dictionary! {
            "Title" => lopdf::Object::string_literal("ebook67-1"),
            "Author" => lopdf::Object::string_literal("China-pub.com"),
            "Publisher" => lopdf::Object::string_literal("测试出版社"),
        });
        document.trailer.set("Info", info_id);
        let mut output = Vec::new();
        document.save_to(&mut output).unwrap();
        output
    }

    #[test]
    fn parses_epub_metadata_and_cover() {
        let bytes = make_epub("书名", "作者甲; 作者乙", "测试出版社", "9787000000000");
        let extracted = parse_epub(&bytes).expect("epub");
        assert_eq!(extracted.metadata.title, "书名");
        assert_eq!(extracted.metadata.authors, "作者甲; 作者乙");
        assert_eq!(extracted.metadata.publisher, "测试出版社");
        assert_eq!(extracted.metadata.isbn, "9787000000000");
        assert_eq!(extracted.metadata.language, "zh");
        assert!(extracted.cover.is_some());
    }

    #[test]
    fn parses_gbk_encoded_opf() {
        let cursor = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        zip.start_file("META-INF/container.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
        )
        .unwrap();
        zip.start_file("OEBPS/content.opf", options).unwrap();
        let opf = r#"<?xml version="1.0" encoding="gbk"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>中文书名</dc:title>
    <dc:creator>作者</dc:creator>
    <dc:publisher>出版社</dc:publisher>
    <dc:language>zh</dc:language>
  </metadata>
  <manifest/>
  <spine/>
</package>"#;
        let (encoded, _, _) = encoding_rs::GBK.encode(opf);
        zip.write_all(&encoded).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let extracted = parse_epub(&bytes).expect("epub");
        assert_eq!(extracted.metadata.title, "中文书名");
        assert_eq!(extracted.metadata.authors, "作者");
        assert_eq!(extracted.metadata.publisher, "出版社");
    }

    #[test]
    fn parses_pdf_page_count() {
        let bytes = make_pdf();
        let info = parse_pdf(&bytes).expect("pdf");
        assert_eq!(info.page_count, 1);
        assert_eq!(info.publisher, "测试出版社");
    }

    #[test]
    fn corrupt_pdf_fails() {
        assert!(parse_pdf(b"not a pdf").is_err());
    }

    #[test]
    fn pdf_uses_filename_when_metadata_is_generic() {
        let extracted = extract("pdf", &make_pdf(), "TCP-IP详解卷1协议.pdf").expect("pdf");
        assert_eq!(extracted.metadata.title, "TCP-IP详解卷1协议");
        assert_eq!(extracted.page_count, 1);
        assert!(is_supported("说明.txt"));
    }

    #[test]
    fn filename_samples() {
        let samples = [
            ("二十四诗品 续诗品.epub", "二十四诗品 续诗品", ""),
            (
                "财富与贫困：国民财富的创造和企业家精神.epub",
                "财富与贫困：国民财富的创造和企业家精神",
                "",
            ),
            ("魔术师.epub", "魔术师", ""),
            (
                "遗珠偶拾：中国现代文学史札记 (郜元宝).pdf",
                "遗珠偶拾：中国现代文学史札记",
                "郜元宝",
            ),
            (
                "行为主义 (科学素养文库·科学元典丛书).epub",
                "行为主义 (科学素养文库·科学元典丛书)",
                "",
            ),
            ("人类在自然界的位置(全译本).azw3", "人类在自然界的位置(全译本)", ""),
            (
                "红楼梦索隐(中) ([清]曹雪芹, 高鹗著； 王梦阮 沈瓶庵索隐).pdf",
                "红楼梦索隐(中)",
                "曹雪芹, 高鹗著； 王梦阮 沈瓶庵索隐",
            ),
            (
                "世界文学史 上 插图本 (（英）德林瓦特主编).pdf",
                "世界文学史 上 插图本",
                "德林瓦特",
            ),
            (
                "读书的风景大学生活之春花秋月(增订版) (陈平原).pdf",
                "读书的风景大学生活之春花秋月(增订版)",
                "陈平原",
            ),
            (
                "中国现代文学三十年 (2018修订本) (钱理群, 温儒敏, 吴福辉).pdf",
                "中国现代文学三十年 (2018修订本)",
                "钱理群, 温儒敏, 吴福辉",
            ),
            ("某书名（全译本）.pdf", "某书名（全译本）", ""),
            ("某书名（作者甲）.pdf", "某书名", "作者甲"),
            ("某书名（丛书名）.pdf", "某书名（丛书名）", ""),
            ("某书名（作者甲） (Z-Library).pdf", "某书名", "作者甲"),
            (
                "C++之父给你的C++学习路线图（套装共5册） (本贾尼·斯特劳斯特鲁普(Bjarne Stroustrup)) (Z-Library).epub",
                "C++之父给你的C++学习路线图（套装共5册）",
                "本贾尼·斯特劳斯特鲁普(Bjarne Stroustrup)",
            ),
        ];
        for (input, title, author) in samples {
            let info = parse_filename(input);
            assert_eq!(info.title, title, "title for {input}");
            assert_eq!(info.author, author, "author for {input}");
        }
    }

    #[test]
    fn repairs_mojibake_filename() {
        let original = "机器学习 (周志华).azw3";
        let broken: String = original
            .as_bytes()
            .iter()
            .map(|byte| *byte as char)
            .collect();
        let info = parse_filename(&broken);
        assert_eq!(info.title, "机器学习");
        assert_eq!(info.author, "周志华");
    }

    #[test]
    fn work_key_groups_same_title_and_author() {
        assert_eq!(
            work_key("书名（修订版）", "张三 著"),
            work_key("书名(修订版)", "张三")
        );
        assert_ne!(work_key("书名", "张三"), work_key("书名", "李四"));
    }
}
