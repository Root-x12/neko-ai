//! File uploads: images (sent to vision models), PDFs and text files.
//!
//! The file type is decided from the bytes, not from what the browser claims.

use std::path::{Path, PathBuf};

use axum::{
    body::Body,
    extract::{Multipart, Path as UrlPath, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use neko_shared::{Attachment, AttachmentKind};

use crate::{
    auth::AuthUser,
    crypto, db,
    error::{AppError, AppResult},
    llm::Media,
    AppState,
};

pub const MAX_UPLOAD: usize = 20 * 1024 * 1024;
/// Anthropic rejects larger images; keep one limit for every provider.
const MAX_IMAGE: usize = 5 * 1024 * 1024;
const MAX_TEXT_CHARS: usize = 200_000;
const ORPHAN_TTL_SECS: i64 = 24 * 3600;

pub fn uploads_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("uploads")
}

fn sniff_image(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some("image/png")
    } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Decides what a file is. Text must be valid UTF-8 without NUL bytes.
pub fn classify(bytes: &[u8], name: &str) -> Result<(AttachmentKind, String), String> {
    if let Some(mime) = sniff_image(bytes) {
        if bytes.len() > MAX_IMAGE {
            return Err("Images can be at most 5 MB".into());
        }
        return Ok((AttachmentKind::Image, mime.into()));
    }
    if bytes.starts_with(b"%PDF-") {
        return Ok((AttachmentKind::Pdf, "application/pdf".into()));
    }
    if !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok() {
        let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let mime = match ext.as_str() {
            "md" | "markdown" => "text/markdown",
            "csv" => "text/csv",
            "json" => "application/json",
            "html" | "htm" => "text/html",
            _ => "text/plain",
        };
        return Ok((AttachmentKind::Text, mime.into()));
    }
    Err("Unsupported file type. You can attach images (PNG, JPEG, GIF, WebP), PDFs and text/code files.".into())
}

/// Extracts text from a PDF on the blocking pool; malformed PDFs can make the parser panic.
pub async fn pdf_text(bytes: Vec<u8>) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(&bytes))
            .map_err(|_| "the PDF could not be parsed".to_string())?
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn clean_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let clean: String = base.chars().filter(|c| !c.is_control()).take(120).collect();
    if clean.trim().is_empty() {
        "file".into()
    } else {
        clean.trim().to_string()
    }
}

pub async fn upload(State(s): State<AppState>, a: AuthUser, mut form: Multipart) -> AppResult<Json<Attachment>> {
    let mut found = None;
    while let Some(field) = form.next_field().await.map_err(|e| AppError::bad_request(format!("Upload failed: {e}")))? {
        if field.name() != Some("file") {
            continue;
        }
        let name = clean_name(field.file_name().unwrap_or("file"));
        let bytes = field.bytes().await.map_err(|e| AppError::bad_request(format!("Upload failed: {e}")))?;
        found = Some((name, bytes));
        break;
    }
    let (name, bytes) = found.ok_or_else(|| AppError::bad_request("No file in the upload"))?;
    if bytes.is_empty() {
        return Err(AppError::bad_request("The file is empty"));
    }
    if bytes.len() > MAX_UPLOAD {
        return Err(AppError::new(StatusCode::PAYLOAD_TOO_LARGE, "Files can be at most 20 MB"));
    }
    let (kind, mime) = classify(&bytes, &name).map_err(AppError::bad_request)?;
    let text = match kind {
        AttachmentKind::Pdf => {
            Some(pdf_text(bytes.to_vec()).await.unwrap_or_else(|e| format!("(could not extract text: {e})")))
        }
        AttachmentKind::Text => Some(String::from_utf8_lossy(&bytes).into_owned()),
        AttachmentKind::Image => None,
    }
    .map(|t| crate::tools::web::truncate(&t, MAX_TEXT_CHARS));

    let dir = uploads_dir(&s.config.data_dir);
    tokio::fs::create_dir_all(&dir).await.map_err(AppError::internal)?;
    let file_name = format!("{}.bin", crypto::random_token());
    tokio::fs::write(dir.join(&file_name), &bytes).await.map_err(AppError::internal)?;
    let flags = text.as_deref().map(crate::guard::scan_injection).unwrap_or_default();
    let (uid, size, stored) = (a.user.id, bytes.len() as i64, file_name.clone());
    let saved =
        s.db.call(move |c| db::insert_attachment(c, uid, &name, &mime, kind, size, &stored, text.as_deref(), &flags))
            .await;
    match saved {
        Ok(att) => Ok(Json(att)),
        Err(e) => {
            let _ = tokio::fs::remove_file(dir.join(&file_name)).await;
            Err(e.into())
        }
    }
}

pub async fn download(State(s): State<AppState>, a: AuthUser, UrlPath(id): UrlPath<i64>) -> AppResult<Response> {
    let rec =
        s.db.call(move |c| db::get_attachment(c, a.user.id, id)).await?.ok_or_else(|| AppError::not_found("File"))?;
    let path = uploads_dir(&s.config.data_dir).join(&rec.path);
    let bytes = tokio::fs::read(&path).await.map_err(|_| AppError::not_found("File"))?;
    let disposition = if rec.info.kind == AttachmentKind::Image { "inline" } else { "attachment" };
    let fname: String = rec.info.name.chars().filter(|c| c.is_ascii_graphic() && *c != '"' || *c == ' ').collect();
    Ok((
        [
            (
                header::CONTENT_TYPE,
                if rec.info.kind == AttachmentKind::Text {
                    "text/plain; charset=utf-8".to_string()
                } else {
                    rec.info.mime.clone()
                },
            ),
            (header::CONTENT_DISPOSITION, format!("{disposition}; filename=\"{fname}\"")),
            (header::CACHE_CONTROL, "private, max-age=86400".to_string()),
            (header::CONTENT_SECURITY_POLICY, "sandbox; default-src 'none'".to_string()),
        ],
        Body::from(bytes),
    )
        .into_response())
}

/// Removes an upload that hasn't been sent yet (the × on a pending attachment).
pub async fn remove(State(s): State<AppState>, a: AuthUser, UrlPath(id): UrlPath<i64>) -> AppResult<StatusCode> {
    let path = s.db.call(move |c| db::delete_attachment(c, a.user.id, id)).await?;
    match path {
        Some(p) => {
            let _ = tokio::fs::remove_file(uploads_dir(&s.config.data_dir).join(p)).await;
            Ok(StatusCode::NO_CONTENT)
        }
        None => Err(AppError::not_found("Pending file")),
    }
}

pub async fn remove_files(data_dir: &Path, paths: Vec<String>) {
    let dir = uploads_dir(data_dir);
    for p in paths {
        let _ = tokio::fs::remove_file(dir.join(p)).await;
    }
}

/// Deletes unsent uploads older than a day and files no row points to.
pub async fn sweep(s: &AppState) {
    let cutoff = db::now() - ORPHAN_TTL_SECS;
    if let Ok(paths) = s.db.call(move |c| db::sweep_orphan_attachments(c, cutoff)).await {
        remove_files(&s.config.data_dir, paths).await;
    }
    let dir = uploads_dir(&s.config.data_dir);
    let Ok(known) = s.db.call(|c| db::all_attachment_paths(c)).await else { return };
    let Ok(mut entries) = tokio::fs::read_dir(&dir).await else { return };
    while let Ok(Some(e)) = entries.next_entry().await {
        let name = e.file_name().to_string_lossy().into_owned();
        let old = e.metadata().await.ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok());
        // Grace period so a file being uploaded right now isn't touched.
        if !known.contains(&name) && old.is_some_and(|d| d.as_secs() > 3600) {
            let _ = tokio::fs::remove_file(e.path()).await;
        }
    }
}

/// Loads a message's files for the provider request.
pub async fn load_media(data_dir: &Path, recs: &[db::AttachmentRecord]) -> Vec<Media> {
    let dir = uploads_dir(data_dir);
    let mut out = Vec::new();
    for r in recs {
        let b64 = match r.info.kind {
            AttachmentKind::Text => String::new(),
            _ => match tokio::fs::read(dir.join(&r.path)).await {
                Ok(bytes) => B64.encode(bytes),
                Err(e) => {
                    tracing::warn!("attachment {} missing on disk: {e}", r.info.id);
                    continue;
                }
            },
        };
        out.push(Media {
            kind: r.info.kind,
            name: r.info.name.clone(),
            mime: r.info.mime.clone(),
            b64,
            text: r.text.clone(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_content() {
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0];
        assert_eq!(classify(&png, "x.txt").unwrap(), (AttachmentKind::Image, "image/png".into()));
        assert_eq!(classify(b"%PDF-1.7 ...", "a.png").unwrap().0, AttachmentKind::Pdf);
        assert_eq!(
            classify("fn main() {}".as_bytes(), "main.rs").unwrap(),
            (AttachmentKind::Text, "text/plain".into())
        );
        assert_eq!(classify(b"# hi", "notes.md").unwrap().1, "text/markdown");
        assert!(classify(&[0x00, 0x01, 0xff], "a.bin").is_err());
        assert!(classify(&[b'M', b'Z', 0x90, 0x00], "a.exe").is_err());
    }

    #[test]
    fn cleans_names() {
        assert_eq!(clean_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_name("C:\\Users\\me\\cat.png"), "cat.png");
        assert_eq!(clean_name("\u{0}"), "file");
    }

    #[tokio::test]
    async fn bad_pdf_does_not_panic() {
        assert!(pdf_text(b"%PDF-1.4 garbage".to_vec()).await.is_err());
    }
}
