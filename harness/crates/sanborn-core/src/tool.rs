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
        Self { allow_write: true, allow_execute: true }
    }

    pub fn read_only() -> Self {
        Self { allow_write: false, allow_execute: false }
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

/// Per-run state shared by tools: the workspace root they are confined to.
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
        Ok(Self { workspace })
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Resolves a user-supplied path inside the workspace, following symlinks
    /// for the portion that exists. Rejects anything that escapes the workspace.
    pub fn resolve(&self, path: &str) -> Result<PathBuf> {
        let raw = Path::new(path);
        let joined = if raw.is_absolute() { raw.to_path_buf() } else { self.workspace.join(raw) };

        let mut normalized = PathBuf::new();
        for component in joined.components() {
            match component {
                Component::ParentDir => {
                    normalized.pop();
                }
                Component::CurDir => {}
                other => normalized.push(other.as_os_str()),
            }
        }

        let mut existing = normalized.as_path();
        let mut rest = Vec::new();
        while !existing.exists() {
            match (existing.file_name(), existing.parent()) {
                (Some(name), Some(parent)) => {
                    rest.push(name.to_os_string());
                    existing = parent;
                }
                _ => break,
            }
        }
        let mut resolved = existing.canonicalize().unwrap_or_else(|_| existing.to_path_buf());
        for name in rest.iter().rev() {
            resolved.push(name);
        }

        if !resolved.starts_with(&self.workspace) {
            bail!("path `{path}` is outside the workspace");
        }
        Ok(resolved)
    }

    /// Workspace-relative form of a path, for display to the model.
    pub fn display(&self, path: &Path) -> String {
        let rel = path.strip_prefix(&self.workspace).unwrap_or(path);
        let s = rel.to_string_lossy();
        if s.is_empty() { ".".to_string() } else { s.into_owned() }
    }
}
