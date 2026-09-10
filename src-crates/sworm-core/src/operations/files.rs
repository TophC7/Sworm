use crate::errors::ApiError;
use crate::events::HostEvent;
use crate::host::Host;
use crate::services::folders::{normalize_absolute_path, resolve_folder};
use std::collections::HashMap;
use std::path::Path;
use sworm_protocol::files::{DirEntry, FilePasteCollision, FilePasteMapping, PathList};

/// Read the contents of a file inside a project.
impl Host {
    pub async fn file_read(
        &self,
        project_path: String,
        file_path: String,
    ) -> Result<String, ApiError> {
        let files = std::sync::Arc::clone(&self.files);
        tokio::task::spawn_blocking(move || files.read(Path::new(&project_path), &file_path))
            .await
            .map_err(|error| ApiError::Internal(error.to_string()))?
    }

    pub async fn file_read_limited(
        &self,
        project_path: String,
        file_path: String,
        max_bytes: usize,
    ) -> Result<String, ApiError> {
        let files = std::sync::Arc::clone(&self.files);
        tokio::task::spawn_blocking(move || {
            files.read_limited(Path::new(&project_path), &file_path, max_bytes)
        })
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))?
    }

    /// Write content to a file inside a project.
    pub async fn file_write(
        &self,
        project_path: String,
        file_path: String,
        content: String,
    ) -> Result<(), ApiError> {
        self.files
            .write(Path::new(&project_path), &file_path, &content)
    }

    /// Create a directory inside a project.
    pub async fn file_create_dir(
        &self,
        project_path: String,
        dir_path: String,
    ) -> Result<(), ApiError> {
        self.files.create_dir(Path::new(&project_path), &dir_path)
    }

    /// Rename a file inside a project.
    pub async fn file_rename(
        &self,
        project_path: String,
        old_path: String,
        new_path: String,
    ) -> Result<(), ApiError> {
        let project = resolve_folder(&project_path)?;
        self.files.validate_path(&old_path)?;
        self.files.validate_path(&new_path)?;
        let old_abs = normalize_absolute_path(&project.join(&old_path));
        self.files.rename(&project, &old_path, &new_path)?;
        let new_abs = normalize_absolute_path(&project.join(&new_path));
        (self.events)(HostEvent::FileMoved {
            folder_path: project,
            old_path: old_abs,
            new_path: new_abs,
            replace_destination: false,
        })
        .map_err(ApiError::Internal)
    }

    /// Paste files into a target directory inside the project.
    /// `op` is "copy" or "cut". Sources are absolute paths from the clipboard.
    /// Returns each transferred source and its new project-relative path.
    pub async fn file_paste(
        &self,
        project_path: String,
        target_dir: String,
        op: String,
        sources: Vec<String>,
        collision_policy: String,
        rename_map: Option<HashMap<String, String>>,
    ) -> Result<Vec<FilePasteMapping>, ApiError> {
        let project = resolve_folder(&project_path)?;
        let source_paths = if op == "cut" {
            sources
                .iter()
                .map(|source| {
                    Ok((
                        source.clone(),
                        normalize_absolute_path(&std::path::absolute(source)?),
                    ))
                })
                .collect::<Result<HashMap<_, _>, ApiError>>()?
        } else {
            HashMap::new()
        };
        let rename_map = rename_map.unwrap_or_default();
        let mappings = self.files.paste(
            &project,
            &target_dir,
            &op,
            &sources,
            &collision_policy,
            &rename_map,
        )?;
        if op == "cut" {
            for mapping in &mappings {
                let source_abs = source_paths
                    .get(&mapping.source)
                    .expect("cut source normalized before paste");
                let new_abs = normalize_absolute_path(&project.join(&mapping.destination));
                (self.events)(HostEvent::FileMoved {
                    folder_path: project.clone(),
                    old_path: source_abs.clone(),
                    new_path: new_abs,
                    replace_destination: true,
                })
                .map_err(ApiError::Internal)?;
            }
        }
        Ok(mappings)
    }

    pub async fn file_paste_collisions(
        &self,
        project_path: String,
        target_dir: String,
        sources: Vec<String>,
    ) -> Result<Vec<FilePasteCollision>, ApiError> {
        self.files
            .paste_collisions(Path::new(&project_path), &target_dir, &sources)
    }

    /// Delete a file or directory inside a project.
    pub async fn file_delete(
        &self,
        project_path: String,
        file_path: String,
    ) -> Result<(), ApiError> {
        let project = resolve_folder(&project_path)?;
        self.files.validate_path(&file_path)?;
        let abs = normalize_absolute_path(&project.join(&file_path));
        self.files.delete(&project, &file_path)?;
        (self.events)(HostEvent::FileDeleted(abs)).map_err(ApiError::Internal)
    }

    /// List one directory as the explorer renders it. `dir_path` is
    /// project-relative; "" is the project root.
    pub async fn files_read_dir(
        &self,
        project_path: String,
        dir_path: String,
        show_hidden: bool,
    ) -> Result<Vec<DirEntry>, ApiError> {
        let generation = *self.settings_generation.lock();
        let files = std::sync::Arc::clone(&self.files);
        tokio::task::spawn_blocking(move || {
            files.read_dir(Path::new(&project_path), &dir_path, show_hidden, generation)
        })
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))?
    }

    /// Flat list of searchable file paths, for Quick Open and the sidebar filter.
    /// The walk visits the whole project, so it runs on a blocking worker rather
    /// than on the async runtime thread.
    pub async fn files_list_paths(
        &self,
        project_path: String,
        show_hidden: bool,
    ) -> Result<PathList, ApiError> {
        let generation = *self.settings_generation.lock();
        let files = std::sync::Arc::clone(&self.files);
        tokio::task::spawn_blocking(move || {
            files.list_paths(Path::new(&project_path), show_hidden, generation)
        })
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))?
    }

    /// Watch exactly the directories the explorer currently renders. A watcher
    /// failure costs freshness, never the listing itself, so it is logged rather
    /// than surfaced.
    pub async fn files_watch_dirs(
        &self,
        subscriber_id: String,
        project_path: String,
        dirs: Vec<String>,
    ) -> Result<(), ApiError> {
        if let Err(error) = self.file_watchers.sync(
            std::sync::Arc::clone(&self.events),
            &subscriber_id,
            Path::new(&project_path),
            &dirs,
        ) {
            tracing::warn!(folder = %project_path, %error, "explorer watcher unavailable");
        }
        Ok(())
    }
}
