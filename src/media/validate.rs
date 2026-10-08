use std::io::{Cursor, Read};

use crate::error::AppError;

pub const KIND_IMAGE: &str = "image";
pub const KIND_FILE: &str = "file";
pub const MAX_ATTACHMENTS: usize = 5;

/// First-release allowlist. Legacy OLE (`.doc` / `.xls` / `.ppt`) is omitted:
/// signatures overlap, malware is common, and mobile preview is poor.
/// SVG/HTML are omitted (script injection). Executables are rejected.
#[derive(Debug, Clone, Copy)]
pub struct AllowedFile {
    pub extension: &'static str,
    pub content_type: &'static str,
    pub kind: &'static str,
}

pub const ALLOWED: &[AllowedFile] = &[
    AllowedFile {
        extension: "png",
        content_type: "image/png",
        kind: KIND_IMAGE,
    },
    AllowedFile {
        extension: "jpg",
        content_type: "image/jpeg",
        kind: KIND_IMAGE,
    },
    AllowedFile {
        extension: "jpeg",
        content_type: "image/jpeg",
        kind: KIND_IMAGE,
    },
    AllowedFile {
        extension: "gif",
        content_type: "image/gif",
        kind: KIND_IMAGE,
    },
    AllowedFile {
        extension: "webp",
        content_type: "image/webp",
        kind: KIND_IMAGE,
    },
    AllowedFile {
        extension: "pdf",
        content_type: "application/pdf",
        kind: KIND_FILE,
    },
    AllowedFile {
        extension: "docx",
        content_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        kind: KIND_FILE,
    },
    AllowedFile {
        extension: "xlsx",
        content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        kind: KIND_FILE,
    },
    AllowedFile {
        extension: "pptx",
        content_type: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        kind: KIND_FILE,
    },
];

pub fn sanitize_file_name(raw: &str) -> Result<String, AppError> {
    let normalized = raw.replace('\\', "/");
    let base = normalized
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .trim()
        .trim_start_matches('.');
    let mut cleaned: String = base
        .chars()
        .filter(|ch| !ch.is_control() && *ch != '\0')
        .map(|ch| {
            if matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                ch
            }
        })
        .collect();
    if cleaned.contains("..") {
        return Err(AppError::validation("file name is invalid"));
    }
    cleaned = cleaned.trim().to_string();
    if cleaned.len() > 255 {
        cleaned.truncate(255);
    }
    if cleaned.is_empty() {
        return Err(AppError::validation("file name is required"));
    }
    Ok(cleaned)
}

pub fn detect_payload(
    file_name: &str,
    declared_type: Option<&str>,
    bytes: &[u8],
    image_only: bool,
) -> Result<AllowedFile, AppError> {
    if bytes.is_empty() {
        return Err(AppError::validation("file is empty"));
    }
    let display = sanitize_file_name(file_name)?;
    let ext = display
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let by_ext = ALLOWED
        .iter()
        .find(|item| item.extension == ext)
        .ok_or_else(|| AppError::validation("file type is not allowed"))?;
    if image_only && by_ext.kind != KIND_IMAGE {
        return Err(AppError::validation("profile pictures must be images"));
    }

    let detected = detect_kind(bytes)
        .ok_or_else(|| AppError::validation("file content could not be identified"))?;
    if detected.content_type != by_ext.content_type {
        return Err(AppError::validation(
            "file extension does not match file contents",
        ));
    }
    if let Some(declared) = declared_type {
        let declared = declared.split(';').next().unwrap_or(declared).trim();
        if !declared.is_empty()
            && !declared.eq_ignore_ascii_case("application/octet-stream")
            && !declared.eq_ignore_ascii_case(by_ext.content_type)
            && !(declared.eq_ignore_ascii_case("image/jpg") && by_ext.content_type == "image/jpeg")
        {
            return Err(AppError::validation(
                "declared content type does not match file contents",
            ));
        }
    }
    Ok(*by_ext)
}

fn detect_kind(bytes: &[u8]) -> Option<AllowedFile> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return ALLOWED.iter().find(|item| item.extension == "png").copied();
    }
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return ALLOWED.iter().find(|item| item.extension == "jpg").copied();
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return ALLOWED.iter().find(|item| item.extension == "gif").copied();
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return ALLOWED
            .iter()
            .find(|item| item.extension == "webp")
            .copied();
    }
    if bytes.starts_with(b"%PDF") {
        return ALLOWED.iter().find(|item| item.extension == "pdf").copied();
    }
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return detect_ooxml(bytes);
    }
    if bytes.len() >= 8
        && bytes[0] == 0xD0
        && bytes[1] == 0xCF
        && bytes[2] == 0x11
        && bytes[3] == 0xE0
    {
        return None;
    }
    if looks_like_script(bytes) {
        return None;
    }
    None
}

fn looks_like_script(bytes: &[u8]) -> bool {
    let prefix = std::str::from_utf8(&bytes[..bytes.len().min(64)]).unwrap_or("");
    let lower = prefix.to_ascii_lowercase();
    lower.contains("<script")
        || lower.starts_with("#!")
        || lower.starts_with("<?php")
        || lower.starts_with("mz")
}

fn detect_ooxml(bytes: &[u8]) -> Option<AllowedFile> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    let mut names = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).ok()?;
        names.push(file.name().replace('\\', "/").to_ascii_lowercase());
        let mut buf = [0u8; 16];
        let _ = file.read(&mut buf);
    }
    if names.iter().any(|n| n.starts_with("word/")) {
        return ALLOWED
            .iter()
            .find(|item| item.extension == "docx")
            .copied();
    }
    if names.iter().any(|n| n.starts_with("xl/")) {
        return ALLOWED
            .iter()
            .find(|item| item.extension == "xlsx")
            .copied();
    }
    if names.iter().any(|n| n.starts_with("ppt/")) {
        return ALLOWED
            .iter()
            .find(|item| item.extension == "pptx")
            .copied();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal_names() {
        let name = sanitize_file_name("..\\..\\etc\\passwd").unwrap();
        assert_eq!(name, "passwd");
        assert!(sanitize_file_name("ok..hidden").is_err());
    }

    #[test]
    fn png_magic_matches_extension() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&[0, 0, 0, 0]);
        let detected = detect_payload("pic.PNG", Some("image/png"), &png, true).unwrap();
        assert_eq!(detected.content_type, "image/png");
    }

    #[test]
    fn extension_mismatch_is_rejected() {
        let pdf = b"%PDF-1.4 minimal";
        let err = detect_payload("notes.png", Some("image/png"), pdf, false).unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
    }
}
