use std::path::{Path, PathBuf};

/// Workspace abstraction for v1.
///
/// Even though v1 only has local workspaces, the trait boundary
/// exists now so v2 can add WorktreeWorkspace and SshWorkspace
/// without restructuring existing code.
#[allow(dead_code)]
pub trait WorkspaceBackend {
    fn workspace_root(&self) -> &Path;
    fn display_label(&self) -> String;
}

/// Local workspace: just a path on disk (the project root).
#[allow(dead_code)]
pub struct LocalWorkspace {
    pub project_root: PathBuf,
}

#[allow(dead_code)]
impl LocalWorkspace {
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }
}

impl WorkspaceBackend for LocalWorkspace {
    fn workspace_root(&self) -> &Path {
        &self.project_root
    }

    fn display_label(&self) -> String {
        self.project_root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.project_root.to_string_lossy().to_string())
    }
}
