//! Files a user attaches to a chat message: screenshots, logs, PDFs, any binary.
//!
//! An attachment is copied into the chat's runtime folder (`<chat>/attachments/`) when its
//! message is sent, so the agent reads a copy that stays put for the chat's life, and is
//! deleted with the chat. Images go to the model natively where the provider takes them
//! (Claude: base64 image blocks on a stream-json message; Codex: `--image`); every
//! attachment is also listed in the message with its path, so the agent can read it with
//! its own tools (a PDF, a log, an image too large to inline) or copy it into the project.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use base64::Engine;
use serde::{Deserialize, Serialize};

/// The folder inside a chat's runtime folder that holds its attachments.
pub const CHAT_ATTACHMENTS_DIR: &str = "attachments";
/// The largest file that can be attached.
pub const CHAT_ATTACHMENT_MAX_BYTES: u64 = 100 * 1024 * 1024;
/// The largest image Claude takes inline (its API caps an image at 5 MB of base64); larger
/// ones are passed by path only.
pub const CLAUDE_INLINE_IMAGE_MAX_BYTES: u64 = 3_750_000;

/// A file attached to a user message, as stored with the transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatAttachment {
    /// The file's name as the user attached it (`screenshot.png`).
    pub name: String,
    /// The chat's own copy of the file.
    pub path: PathBuf,
    /// Its MIME type by name (`image/png`); `application/octet-stream` when unknown.
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub size_bytes: u64,
}

impl ChatAttachment {
    /// An image type the providers can show the model (PNG, JPEG, GIF, WebP).
    pub fn is_image(&self) -> bool {
        is_model_image_type(&self.mime_type)
    }
}

/// A file to attach: where it is now and the copy the chat keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatAttachmentUpload {
    pub source: PathBuf,
    pub attachment: ChatAttachment,
}

/// Whether models take images of `mime_type` directly.
pub fn is_model_image_type(mime_type: &str) -> bool {
    matches!(
        mime_type,
        "image/png" | "image/jpeg" | "image/gif" | "image/webp"
    )
}

/// The MIME type of a file by its extension; `application/octet-stream` when unknown.
pub fn attachment_mime_type(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "tif" | "tiff" => "image/tiff",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "json" => "application/json",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "md" | "markdown" => "text/markdown",
        "txt" | "log" => "text/plain",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

/// Checks the files to attach and picks where the chat keeps each copy (inside
/// `chat_dir/attachments`, under its own name, numbered when the name is taken). Nothing is
/// copied yet ([`stage_chat_attachments`]).
pub fn plan_chat_attachments(
    chat_dir: &Path,
    sources: &[PathBuf],
) -> Result<Vec<ChatAttachmentUpload>, String> {
    let dir = chat_dir.join(CHAT_ATTACHMENTS_DIR);
    let mut taken = BTreeSet::new();
    let mut uploads = Vec::with_capacity(sources.len());
    for source in sources {
        let name = display_name(source);
        let meta = fs::metadata(source).map_err(|err| format!("Couldn't read {name}: {err}"))?;
        if !meta.is_file() {
            return Err(format!("{name} is a folder; attach the files inside it"));
        }
        if meta.len() > CHAT_ATTACHMENT_MAX_BYTES {
            return Err(format!(
                "{name} is {}; attachments can be up to {}",
                format_bytes(meta.len()),
                format_bytes(CHAT_ATTACHMENT_MAX_BYTES)
            ));
        }
        let path = unique_path(&dir, &safe_file_name(&name), &mut taken);
        uploads.push(ChatAttachmentUpload {
            source: source.clone(),
            attachment: ChatAttachment {
                mime_type: attachment_mime_type(Path::new(&name)).to_owned(),
                name,
                path,
                size_bytes: meta.len(),
            },
        });
    }
    Ok(uploads)
}

/// Copies each attachment to the chat's folder (skipping ones already there).
pub(crate) fn stage_chat_attachments(uploads: &[ChatAttachmentUpload]) -> Result<(), String> {
    for upload in uploads {
        let target = &upload.attachment.path;
        if target.is_file() {
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Couldn't create {}: {err}", parent.display()))?;
        }
        fs::copy(&upload.source, target)
            .map_err(|err| format!("Couldn't attach {}: {err}", upload.attachment.name))?;
    }
    Ok(())
}

/// The message as the agent gets it: the user's text, then a list of what is attached and
/// where to read it.
pub(crate) fn prompt_with_attachments(prompt: &str, attachments: &[ChatAttachment]) -> String {
    if attachments.is_empty() {
        return prompt.to_owned();
    }
    let mut text = prompt.trim_end().to_owned();
    if !text.is_empty() {
        text.push_str("\n\n");
    }
    text.push_str(if attachments.len() == 1 {
        "I attached a file. Read it from this path:"
    } else {
        "I attached files. Read them from these paths:"
    });
    for attachment in attachments {
        text.push_str(&format!(
            "\n- {} ({}, {})",
            attachment.path.display(),
            attachment.mime_type,
            format_bytes(attachment.size_bytes)
        ));
    }
    text
}

/// Images Claude gets inline as image blocks.
pub(crate) fn claude_inline_images(attachments: &[ChatAttachment]) -> Vec<&ChatAttachment> {
    attachments
        .iter()
        .filter(|a| a.is_image() && a.size_bytes <= CLAUDE_INLINE_IMAGE_MAX_BYTES)
        .collect()
}

/// One stream-json user message for Claude Code: the text, then each image as a base64
/// block. Reads the images, so the attachments must be staged.
pub(crate) fn claude_user_message_line(
    text: &str,
    images: &[&ChatAttachment],
) -> Result<String, String> {
    let mut content = vec![serde_json::json!({ "type": "text", "text": text })];
    for image in images {
        let bytes =
            fs::read(&image.path).map_err(|err| format!("Couldn't read {}: {err}", image.name))?;
        content.push(serde_json::json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": image.mime_type,
                "data": base64::engine::general_purpose::STANDARD.encode(bytes),
            },
        }));
    }
    let line = serde_json::json!({
        "type": "user",
        "message": { "role": "user", "content": content },
    });
    Ok(format!("{line}\n"))
}

/// "812 B", "14 KB", "2.4 MB".
pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let value = bytes as f64;
    if value < KB {
        format!("{bytes} B")
    } else if value < MB {
        format!("{:.0} KB", value / KB)
    } else if value < MB * 10.0 {
        format!("{:.1} MB", value / MB)
    } else {
        format!("{:.0} MB", value / MB)
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "file".to_owned())
}

/// A name that is safe as one path component.
fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '\0' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_owned();
    if cleaned.is_empty() {
        "file".to_owned()
    } else {
        cleaned
    }
}

/// `dir/name`, or `dir/name-2.ext`, `-3`, … when a file of that name exists or this batch
/// already took it.
fn unique_path(dir: &Path, name: &str, taken: &mut BTreeSet<String>) -> PathBuf {
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    };
    let mut candidate = name.to_owned();
    let mut n = 2;
    while taken.contains(&candidate) || dir.join(&candidate).exists() {
        candidate = format!("{stem}-{n}{extension}");
        n += 1;
    }
    taken.insert(candidate.clone());
    dir.join(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn plans_copies_under_unique_names_and_rejects_folders_and_missing_files() {
        let temp = TempDir::new().unwrap();
        let a = temp.path().join("one").join("shot.png");
        let b = temp.path().join("two").join("shot.png");
        for path in [&a, &b] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"png").unwrap();
        }
        let chat = temp.path().join("chat");
        fs::create_dir_all(chat.join(CHAT_ATTACHMENTS_DIR)).unwrap();
        fs::write(chat.join(CHAT_ATTACHMENTS_DIR).join("shot.png"), b"old").unwrap();

        let uploads = plan_chat_attachments(&chat, &[a.clone(), b.clone()]).unwrap();
        let names: Vec<_> = uploads
            .iter()
            .map(|u| {
                u.attachment
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["shot-2.png", "shot-3.png"]);
        assert_eq!(uploads[0].attachment.name, "shot.png");
        assert_eq!(uploads[0].attachment.mime_type, "image/png");
        assert_eq!(uploads[0].attachment.size_bytes, 3);
        assert!(uploads[0].attachment.is_image());

        stage_chat_attachments(&uploads).unwrap();
        assert_eq!(fs::read(&uploads[1].attachment.path).unwrap(), b"png");

        assert!(
            plan_chat_attachments(&chat, &[temp.path().join("one")])
                .unwrap_err()
                .contains("folder")
        );
        assert!(plan_chat_attachments(&chat, &[temp.path().join("nope.txt")]).is_err());
    }

    #[test]
    fn prompts_list_attachments_and_claude_messages_inline_images() {
        let temp = TempDir::new().unwrap();
        let image = temp.path().join("a.png");
        fs::write(&image, [1u8, 2, 3]).unwrap();
        let attachments = vec![
            ChatAttachment {
                name: "a.png".to_owned(),
                path: image.clone(),
                mime_type: "image/png".to_owned(),
                size_bytes: 3,
            },
            ChatAttachment {
                name: "log.txt".to_owned(),
                path: temp.path().join("log.txt"),
                mime_type: "text/plain".to_owned(),
                size_bytes: 2048,
            },
        ];
        let prompt = prompt_with_attachments("What broke?", &attachments);
        assert!(
            prompt.starts_with("What broke?\n\nI attached files."),
            "{prompt}"
        );
        assert!(prompt.contains(&format!("- {} (image/png, 3 B)", image.display())));
        assert!(prompt.contains("(text/plain, 2 KB)"));
        assert_eq!(prompt_with_attachments("Hi", &[]), "Hi");

        let images = claude_inline_images(&attachments);
        assert_eq!(images.len(), 1);
        let line = claude_user_message_line("Look", &images).unwrap();
        let value: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(value["type"], "user");
        let content = &value["message"]["content"];
        assert_eq!(content[0]["text"], "Look");
        assert_eq!(content[1]["source"]["media_type"], "image/png");
        assert_eq!(content[1]["source"]["data"], "AQID");
    }

    #[test]
    fn file_names_stay_one_path_component() {
        assert_eq!(safe_file_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(safe_file_name(".hidden"), "hidden");
        assert_eq!(safe_file_name("   "), "file");
        assert_eq!(format_bytes(5 * 1024 * 1024 + 300_000), "5.3 MB");
    }
}
