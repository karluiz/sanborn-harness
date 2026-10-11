use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde_json::Value;

use crate::provider::ToolSpec;

/// What a tool can do. Read tools are safe to run in parallel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Read,
    Write,
    Execute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub allow_write: bool,
    pub allow_execute: bool,
}

impl Policy {
    pub fn full() -> Self {
        Self {
            allow_write: true,
            allow_execute: true,
        }
    }

    pub fn read_only() -> Self {
        Self {
            allow_write: false,
            allow_execute: false,
        }
    }

    pub fn permits(&self, kind: ToolKind) -> bool {
        match kind {
            ToolKind::Read => true,
            ToolKind::Write => self.allow_write,
            ToolKind::Execute => self.allow_execute,
        }
    }
}

impl Default for Policy {
    fn default() -> Self {
        Self::full()
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    fn kind(&self) -> ToolKind;
    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<String>;
}

#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: Vec<Arc<dyn Tool>>,
    index: HashMap<String, usize>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a tool, replacing any existing tool with the same name.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.spec().name;
        if let Some(&i) = self.index.get(&name) {
            self.tools[i] = tool;
        } else {
            self.index.insert(name, self.tools.len());
            self.tools.push(tool);
        }
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.index.get(name).map(|&i| &self.tools[i])
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.iter().map(|t| t.spec()).collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn Tool>> {
        self.tools.iter()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

/// Workspace path checks for tools. Not an OS sandbox or protection against races.
#[derive(Debug, Clone)]
pub struct ToolContext {
    workspace: PathBuf,
}

impl ToolContext {
    pub fn new(workspace: impl AsRef<Path>) -> Result<Self> {
        let workspace = workspace.as_ref();
        let workspace = workspace
            .canonicalize()
            .with_context(|| format!("workspace {} is not accessible", workspace.display()))?;
        if !workspace.is_dir() {
            bail!("workspace must be a directory");
        }
        Ok(Self { workspace })
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Resolves a user-supplied path inside the workspace, following symlinks
    /// for the portion that exists. Rejects anything that escapes the workspace.
    pub fn resolve(&self, path: &str) -> Result<PathBuf> {
        let raw = Path::new(path);
        let relative = if raw.is_absolute() {
            raw.strip_prefix(&self.workspace)
                .context("absolute path is outside the workspace")?
        } else {
            raw
        };
        let mut resolved = self.workspace.clone();
        for component in relative.components() {
            match component {
                Component::ParentDir => {
                    if resolved == self.workspace {
                        bail!("path `{path}` is outside the workspace");
                    }
                    resolved.pop();
                }
                Component::CurDir => {}
                Component::Normal(name) => {
                    resolved.push(name);
                    match std::fs::symlink_metadata(&resolved) {
                        Ok(_) => {
                            resolved = resolved
                                .canonicalize()
                                .with_context(|| format!("cannot resolve path `{path}`"))?;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error).context("cannot inspect workspace path"),
                    }
                }
                _ => bail!("unsupported workspace path"),
            }
            if !resolved.starts_with(&self.workspace) {
                bail!("path `{path}` is outside the workspace");
            }
        }
        Ok(resolved)
    }

    /// Workspace-relative form of a path, for display to the model.
    pub fn display(&self, path: &Path) -> String {
        let rel = path.strip_prefix(&self.workspace).unwrap_or(path);
        let s = rel.to_string_lossy();
        if s.is_empty() {
            ".".to_string()
        } else {
            s.into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_new_paths_and_rejects_escape() {
        let root = tempfile::tempdir().unwrap();
        let ctx = ToolContext::new(root.path()).unwrap();
        assert_eq!(
            ctx.resolve("new/file.txt").unwrap(),
            root.path().join("new/file.txt")
        );
        assert!(ctx.resolve("../outside").is_err());
        assert!(ctx.resolve("/etc/passwd").is_err());
        assert!(ToolContext::new(root.path().join("missing")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_external_and_dangling_symlinks_including_parent_traversal() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("missing"), root.path().join("dangling"))
            .unwrap();
        let ctx = ToolContext::new(root.path()).unwrap();
        for path in ["link/file", "link/../file", "dangling", "dangling/file"] {
            assert!(ctx.resolve(path).is_err(), "{path}");
        }
    }
}
