use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use ignore::WalkBuilder;
use regex::RegexBuilder;
use sanborn_core::{Tool, ToolContext, ToolKind, ToolRegistry, ToolSpec};
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_FILE_BYTES: usize = 128_000;
const MAX_ENTRIES: usize = 2_000;
const MAX_SEARCH_BYTES: usize = 4_000_000;

pub fn workspace_tools() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    for kind in [
        FileTool::Read,
        FileTool::List,
        FileTool::Search,
        FileTool::Write,
    ] {
        registry.register(Arc::new(kind));
    }
    registry
}

#[derive(Clone, Copy)]
enum FileTool {
    Read,
    List,
    Search,
    Write,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathInput {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchInput {
    path: String,
    pattern: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteInput {
    path: String,
    content: String,
    /// Null creates a new file; a string must exactly match existing contents.
    expected_content: Option<String>,
}

fn protected(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(name) => {
            let name = name.to_string_lossy();
            matches!(
                name.as_ref(),
                ".git" | ".ssh" | ".env" | "id_rsa" | "id_ed25519"
            ) || name.starts_with(".env.")
                || name.ends_with(".local")
                || name.ends_with(".pem")
                || name.ends_with(".key")
        }
        _ => false,
    })
}

fn resolve(ctx: &ToolContext, path: &str) -> Result<PathBuf> {
    let resolved = ctx.resolve(path)?;
    let relative = resolved
        .strip_prefix(ctx.workspace())
        .context("path escaped workspace")?;
    if protected(relative) || protected(Path::new(path)) {
        bail!("refusing credential-like or repository-internal path");
    }
    Ok(resolved)
}

fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    if !std::fs::metadata(path)
        .context("cannot inspect file")?
        .is_file()
    {
        bail!("path is not a regular file");
    }
    let file = std::fs::File::open(path).context("cannot open file")?;
    if !file.metadata()?.is_file() {
        bail!("path is not a regular file");
    }
    let mut bytes = Vec::new();
    file.take((MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FILE_BYTES {
        bail!("file exceeds {MAX_FILE_BYTES} bytes; narrow the task");
    }
    Ok(bytes)
}

#[async_trait]
impl Tool for FileTool {
    fn spec(&self) -> ToolSpec {
        let (name, description, input_schema) = match self {
            Self::Read => ("read_file", "Read a UTF-8 regular file, at most 128000 bytes.",
                json!({"type": "object", "additionalProperties": false, "required": ["path"],
                    "properties": {"path": {"type": "string"}}})),
            Self::List => ("list_files", "List visible, non-ignored workspace entries recursively; at most 2000 entries. Does not follow symlinks.",
                json!({"type": "object", "additionalProperties": false, "required": ["path"],
                    "properties": {"path": {"type": "string"}}})),
            Self::Search => ("search_files", "Search a Rust regex in visible, non-ignored UTF-8 files. Limited to 2000 entries and 4 MB; binary and oversized files are reported as skipped.",
                json!({"type": "object", "additionalProperties": false, "required": ["path", "pattern"],
                    "properties": {"path": {"type": "string"}, "pattern": {"type": "string"}}})),
            Self::Write => ("write_file", "Atomically write UTF-8 content, at most 128000 bytes. expected_content=null creates a new file without overwrite; otherwise it must exactly match current contents. Parent directory must exist.",
                json!({"type": "object", "additionalProperties": false,
                    "required": ["path", "content", "expected_content"],
                    "properties": {"path": {"type": "string"}, "content": {"type": "string"},
                        "expected_content": {"type": ["string", "null"]}}})),
        };
        ToolSpec {
            name: name.into(),
            description: description.into(),
            input_schema,
        }
    }

    fn kind(&self) -> ToolKind {
        match self {
            Self::Write => ToolKind::Write,
            _ => ToolKind::Read,
        }
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<String> {
        let tool = *self;
        let ctx = ctx.clone();
        tokio::task::spawn_blocking(move || tool.execute(input, &ctx))
            .await
            .context("workspace tool worker failed")?
    }
}

impl FileTool {
    fn execute(&self, input: Value, ctx: &ToolContext) -> Result<String> {
        match self {
            Self::Read => {
                let input: PathInput =
                    serde_json::from_value(input).context("invalid read_file input")?;
                let path = resolve(ctx, &input.path)?;
                String::from_utf8(read_bytes(&path)?).context("file is not UTF-8 text")
            }
            Self::List | Self::Search => {
                let (path, pattern) = if matches!(self, Self::Search) {
                    let input: SearchInput =
                        serde_json::from_value(input).context("invalid search_files input")?;
                    if input.pattern.is_empty() || input.pattern.len() > 256 {
                        bail!("search pattern must have 1 to 256 bytes");
                    }
                    let regex = RegexBuilder::new(&input.pattern)
                        .size_limit(1_000_000)
                        .build()
                        .context("invalid search regex")?;
                    (resolve(ctx, &input.path)?, Some(regex))
                } else {
                    let input: PathInput =
                        serde_json::from_value(input).context("invalid list_files input")?;
                    (resolve(ctx, &input.path)?, None)
                };
                if !path.exists() {
                    bail!("search/list path does not exist");
                }
                let walker = WalkBuilder::new(&path)
                    .follow_links(false)
                    .max_depth(Some(20))
                    .git_global(false)
                    .git_ignore(true)
                    .git_exclude(true)
                    .require_git(false)
                    .build();
                let mut entries = Vec::new();
                let mut matches = Vec::new();
                let mut bytes_read = 0;
                let mut output_bytes = 0;
                let mut skipped_binary = 0;
                let mut skipped_large = 0;
                let mut limited = false;
                for (count, entry) in walker.enumerate() {
                    if count >= MAX_ENTRIES {
                        limited = true;
                        break;
                    }
                    let entry = entry.context("workspace traversal failed")?;
                    if entry.depth() == 0 && path.is_dir() {
                        continue;
                    }
                    let relative = entry
                        .path()
                        .strip_prefix(ctx.workspace())
                        .context("traversal left workspace")?;
                    if protected(relative) {
                        continue;
                    }
                    let kind = entry.file_type().context("entry has no file type")?;
                    if kind.is_symlink() {
                        continue;
                    }
                    if let Some(regex) = &pattern {
                        if !kind.is_file() {
                            continue;
                        }
                        let safe = resolve(ctx, &ctx.display(entry.path()))?;
                        let size = std::fs::metadata(&safe)?.len();
                        if size > MAX_FILE_BYTES as u64 {
                            skipped_large += 1;
                            continue;
                        }
                        if bytes_read + size as usize > MAX_SEARCH_BYTES {
                            limited = true;
                            break;
                        }
                        let bytes = read_bytes(&safe)?;
                        bytes_read += bytes.len();
                        let text = match String::from_utf8(bytes) {
                            Ok(text) if !text.contains('\0') => text,
                            _ => {
                                skipped_binary += 1;
                                continue;
                            }
                        };
                        for (line, text) in text.lines().enumerate() {
                            if regex.is_match(text) {
                                if output_bytes + text.len() > 24_000 {
                                    limited = true;
                                    break;
                                }
                                output_bytes += text.len();
                                matches.push(json!({"path": ctx.display(&safe), "line": line + 1, "text": text}));
                            }
                        }
                        if limited {
                            break;
                        }
                    } else {
                        entries.push(format!(
                            "{}{}",
                            ctx.display(entry.path()),
                            if kind.is_dir() { "/" } else { "" }
                        ));
                    }
                }
                if pattern.is_some() {
                    Ok(serde_json::to_string(&json!({
                        "matches": matches, "limited": limited,
                        "skipped_binary": skipped_binary, "skipped_oversized": skipped_large,
                        "scope": "visible, non-ignored files; maximum depth 20"
                    }))?)
                } else {
                    entries.sort();
                    Ok(serde_json::to_string(&json!({
                        "entries": entries, "limited": limited, "scope": "visible, non-ignored entries; maximum depth 20"
                    }))?)
                }
            }
            Self::Write => {
                if input.get("expected_content").is_none() {
                    bail!("write_file requires expected_content (null for a new file)");
                }
                let input: WriteInput =
                    serde_json::from_value(input).context("invalid write_file input")?;
                if input.content.len() > MAX_FILE_BYTES {
                    bail!("write content exceeds {MAX_FILE_BYTES} bytes");
                }
                let path = resolve(ctx, &input.path)?;
                let parent = path.parent().context("write path has no parent")?;
                if !parent.is_dir() {
                    bail!("write parent must be an existing directory");
                }
                if let Some(expected) = &input.expected_content {
                    let current = String::from_utf8(read_bytes(&path)?)
                        .context("existing file is not UTF-8")?;
                    if &current != expected {
                        bail!("existing content does not match expected_content; read and replan");
                    }
                } else if path.exists() {
                    bail!("file already exists; read before requesting an overwrite");
                }
                let permissions = if input.expected_content.is_some() {
                    Some(std::fs::metadata(&path)?.permissions())
                } else {
                    None
                };
                let mut temp = tempfile::NamedTempFile::new_in(parent)?;
                temp.write_all(input.content.as_bytes())?;
                if let Some(permissions) = permissions {
                    temp.as_file().set_permissions(permissions)?;
                }
                temp.as_file().sync_all()?;
                // Recheck just before replacement; this is optimistic concurrency, not a file lock.
                if let Some(expected) = &input.expected_content {
                    let path = resolve(ctx, &input.path)?;
                    if String::from_utf8(read_bytes(&path)?)? != *expected {
                        bail!("file changed during write preparation");
                    }
                    temp.persist(&path)
                        .map_err(|error| error.error)
                        .context("atomic replacement failed")?;
                } else {
                    temp.persist_noclobber(&path)
                        .map_err(|error| error.error)
                        .context("atomic file creation failed")?;
                }
                Ok(format!(
                    "Wrote {} bytes to {}",
                    input.content.len(),
                    ctx.display(&path)
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_read_compare_write_and_search() {
        let root = tempfile::tempdir().unwrap();
        let ctx = ToolContext::new(root.path()).unwrap();
        let tools = workspace_tools();
        let write = tools.get("write_file").unwrap();
        write
            .call(
                json!({"path": "hello.txt", "content": "hello", "expected_content": null}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(write
            .call(
                json!({"path": "hello.txt", "content": "bad", "expected_content": null}),
                &ctx
            )
            .await
            .is_err());
        assert!(write
            .call(
                json!({"path": "hello.txt", "content": "bad", "expected_content": "stale"}),
                &ctx
            )
            .await
            .is_err());
        write
            .call(
                json!({"path": "hello.txt", "content": "hello world", "expected_content": "hello"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(
            tools
                .get("read_file")
                .unwrap()
                .call(json!({"path": "hello.txt"}), &ctx)
                .await
                .unwrap(),
            "hello world"
        );
        let found = tools
            .get("search_files")
            .unwrap()
            .call(json!({"path": ".", "pattern": "world"}), &ctx)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&found).unwrap()["matches"][0]["line"],
            1
        );
    }

    #[tokio::test]
    async fn rejects_escape_sensitive_paths_oversize_and_missing_precondition() {
        let root = tempfile::tempdir().unwrap();
        let ctx = ToolContext::new(root.path()).unwrap();
        let tools = workspace_tools();
        for path in ["../outside", ".env", ".git/config", "secret.key"] {
            assert!(tools
                .get("read_file")
                .unwrap()
                .call(json!({"path": path}), &ctx)
                .await
                .is_err());
        }
        std::fs::write(root.path().join("large"), vec![b'a'; MAX_FILE_BYTES + 1]).unwrap();
        assert!(tools
            .get("read_file")
            .unwrap()
            .call(json!({"path": "large"}), &ctx)
            .await
            .is_err());
        assert!(tools
            .get("write_file")
            .unwrap()
            .call(json!({"path": "new", "content": ""}), &ctx)
            .await
            .is_err());
        assert!(!root.path().join("new").exists());
        assert!(tools
            .get("read_file")
            .unwrap()
            .call(json!({"path": "."}), &ctx)
            .await
            .is_err());
    }
}
