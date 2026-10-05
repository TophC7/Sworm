use crate::errors::ApiError;
use crate::services::explorer_filter::{ExplorerFilter, IgnoreChain};
use crate::services::settings_resolution::resolve_effective_settings_for_folder_path;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use sworm_protocol::files::{
    DirEntry, FileContent, FilePasteCollision, FilePasteMapping, PathList,
};
#[cfg(test)]
use sworm_protocol::settings::ExplorerSettings;

const MAX_DEPTH: usize = 50;
/// Bounds the single-child chain a compacted row may represent, so a
/// pathological deep chain can't turn one listing into unbounded work.
const MAX_COMPACT_HOPS: usize = 32;
/// Ceiling for the flat search list behind Quick Open and the sidebar filter.
/// Search only — directory listings are unbounded — and hitting it is reported
/// to the UI rather than silently dropping paths.
const MAX_SEARCH_PATHS: usize = 200_000;

pub struct FileService {
    /// Compiled explorer filter per project, rebuilt when the settings
    /// generation moves so an edited `settings.jsonc` takes effect at once.
    filters: Mutex<HashMap<PathBuf, (u64, Arc<ExplorerFilter>)>>,
}

/// Bounded descriptor-backed read. EOF validates both descriptor and path identity.
pub struct FileReadStream {
    file: File,
    path: PathBuf,
    identity: StatIdentity,
    bytes: u64,
    hash: Sha256,
}

/// Metadata identity of a file: any replace, truncate, or write moves it.
/// Compared numerically per chunk; formatted only as a version string.
#[derive(Clone, Copy, PartialEq, Eq)]
struct StatIdentity {
    dev: u64,
    ino: u64,
    len: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
}

impl StatIdentity {
    fn of(metadata: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            dev: metadata.dev(),
            ino: metadata.ino(),
            len: metadata.len(),
            mtime: (metadata.mtime(), metadata.mtime_nsec()),
            ctime: (metadata.ctime(), metadata.ctime_nsec()),
        }
    }
}

impl std::fmt::Display for StatIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}:{}:{}:{}:{}",
            self.dev, self.ino, self.len, self.mtime.0, self.mtime.1, self.ctime.0, self.ctime.1
        )
    }
}

impl FileReadStream {
    /// Size the stream was opened at; the identity check pins it.
    pub fn size(&self) -> u64 {
        self.identity.len
    }

    /// Append at most one chunk to `out`, returning how many bytes were read.
    /// `0` is EOF, returned only once the full opened size was read unchanged.
    /// Reads into `out`'s spare capacity, so a reused or presized buffer costs
    /// neither an allocation nor a zero-fill per chunk.
    pub fn read_chunk(&mut self, out: &mut Vec<u8>) -> Result<usize, ApiError> {
        use sworm_protocol::rpc::MAX_FILE_CHUNK_BYTES;
        let size = self.size();
        // At EOF this still asks for one byte, so growth past `size` is caught.
        let limit = size
            .saturating_sub(self.bytes)
            .clamp(1, MAX_FILE_CHUNK_BYTES as u64);
        let start = out.len();
        let count = (&self.file)
            .take(limit)
            .read_to_end(out)
            .map_err(|error| ApiError::Io(error.to_string()))?;
        self.bytes += count as u64;
        if self.bytes > size {
            return Err(ApiError::InvalidArgument(
                "File grew during read".to_owned(),
            ));
        }
        let current = StatIdentity::of(
            &self
                .file
                .metadata()
                .map_err(|error| ApiError::Io(error.to_string()))?,
        );
        let path_identity = StatIdentity::of(
            &std::fs::metadata(&self.path).map_err(|error| ApiError::Io(error.to_string()))?,
        );
        if current != self.identity
            || path_identity != self.identity
            || (count == 0 && self.bytes != size)
        {
            return Err(ApiError::Conflict {
                current_version: path_identity.to_string(),
            });
        }
        self.hash.update(&out[start..]);
        Ok(count)
    }

    /// Content hash, meaningful only after `read_chunk` successfully returns EOF.
    pub fn version(&self) -> String {
        format!("{:x}", self.hash.clone().finalize())
    }
}

impl FileService {
    pub fn new() -> Self {
        Self {
            filters: Mutex::new(HashMap::new()),
        }
    }

    pub fn stat(
        &self,
        project_path: &Path,
        file_path: &str,
    ) -> Result<sworm_protocol::files::FileStat, ApiError> {
        validate_path(file_path)?;
        let metadata = std::fs::metadata(project_path.join(file_path))
            .map_err(|error| ApiError::Io(format!("Failed to stat {file_path}: {error}")))?;
        Ok(sworm_protocol::files::FileStat {
            size: metadata.len(),
            version: StatIdentity::of(&metadata).to_string(),
            regular: metadata.is_file(),
        })
    }

    /// Opens for streaming at `version`, the stat identity the caller approved.
    /// This descriptor check is the one open-time validation: it pins size and
    /// identity for every later chunk.
    pub fn open_read_stream(
        &self,
        project_path: &Path,
        file_path: &str,
        version: &str,
    ) -> Result<FileReadStream, ApiError> {
        use sworm_protocol::rpc::MAX_STREAM_FILE_BYTES;
        validate_path(file_path)?;
        let path = project_path.join(file_path);
        let (file, metadata) = Self::open_regular(&path, file_path)?;
        if metadata.len() > MAX_STREAM_FILE_BYTES as u64 {
            return Err(ApiError::TooLarge {
                size: metadata.len(),
                limit: MAX_STREAM_FILE_BYTES as u64,
            });
        }
        let identity = StatIdentity::of(&metadata);
        let current_version = identity.to_string();
        if current_version != version {
            return Err(ApiError::Conflict { current_version });
        }
        Ok(FileReadStream {
            file,
            path,
            identity,
            bytes: 0,
            hash: Sha256::new(),
        })
    }

    /// Read a whole file inside a project, with the version of the bytes it
    /// was read from. Files over `MAX_WHOLE_FILE_BYTES` are refused with
    /// `TooLarge` before their contents are allocated; those stream instead.
    pub fn read(&self, project_path: &Path, file_path: &str) -> Result<FileContent, ApiError> {
        use sworm_protocol::rpc::MAX_WHOLE_FILE_BYTES;
        let limit = MAX_WHOLE_FILE_BYTES as u64;
        validate_path(file_path)?;
        let abs = project_path.join(file_path);
        let (file, metadata) = Self::open_regular(&abs, file_path)?;
        if metadata.len() > limit {
            return Err(ApiError::TooLarge {
                size: metadata.len(),
                limit,
            });
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        // One byte past the limit is enough to catch a file that grew since the fstat.
        file.take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| ApiError::Io(format!("Failed to read {file_path}: {error}")))?;
        if bytes.len() as u64 > limit {
            return Err(ApiError::TooLarge {
                size: bytes.len() as u64,
                limit,
            });
        }
        file_content(bytes, file_path)
    }

    /// Open without blocking on FIFOs/devices, then validate the opened descriptor
    /// so a path swap between check and open cannot hand back a non-regular file.
    /// `Ok(None)` means the path does not exist.
    fn open_regular_opt(
        abs: &Path,
        file_path: &str,
    ) -> Result<Option<(File, std::fs::Metadata)>, ApiError> {
        use std::os::unix::fs::OpenOptionsExt;

        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(abs)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(ApiError::Io(format!("Failed to read {file_path}: {error}"))),
        };
        let metadata = file
            .metadata()
            .map_err(|error| ApiError::Io(format!("Failed to read {file_path}: {error}")))?;
        if !metadata.file_type().is_file() {
            return Err(ApiError::InvalidArgument(format!(
                "{file_path} is not a regular file"
            )));
        }
        Ok(Some((file, metadata)))
    }

    fn open_regular(abs: &Path, file_path: &str) -> Result<(File, std::fs::Metadata), ApiError> {
        Self::open_regular_opt(abs, file_path)?.ok_or_else(|| {
            // Same text the plain `open` produced before the tolerant variant existed.
            ApiError::Io(format!(
                "Failed to read {file_path}: {}",
                std::io::Error::from_raw_os_error(libc::ENOENT)
            ))
        })
    }

    /// Version of the bytes on disk, or `None` when nothing is there. Hashes
    /// as it reads so a save never has to hold the whole on-disk file.
    fn current_version(abs: &Path, file_path: &str) -> Result<Option<String>, ApiError> {
        let Some((mut file, _)) = Self::open_regular_opt(abs, file_path)? else {
            return Ok(None);
        };
        let mut hasher = Sha256::new();
        std::io::copy(&mut file, &mut hasher)
            .map_err(|error| ApiError::Io(format!("Failed to read {file_path}: {error}")))?;
        Ok(Some(format!("{:x}", hasher.finalize())))
    }

    /// Write content to a file inside a project, returning the version of the
    /// bytes just written so the caller's next write can check against it.
    /// The file must already exist or its parent directory must exist.
    ///
    /// `expected_version` is the version the caller last read. Anything other
    /// than exactly those bytes on disk refuses the write: a different version
    /// is `ApiError::Conflict`, and a file deleted since the read is
    /// `ApiError::Deleted` — recreating it would silently undo the deletion,
    /// so the caller has to ask for that with an unconditional write.
    /// `None` overwrites, or creates, unconditionally.
    pub fn write(
        &self,
        project_path: &Path,
        file_path: &str,
        content: &str,
        expected_version: Option<&str>,
    ) -> Result<String, ApiError> {
        validate_path(file_path)?;
        let abs = project_path.join(file_path);
        if let Some(expected) = expected_version {
            match Self::current_version(&abs, file_path)? {
                Some(current) if current == expected => {}
                Some(current) => {
                    return Err(ApiError::Conflict {
                        current_version: current,
                    })
                }
                None => {
                    return Err(ApiError::Deleted {
                        path: file_path.to_string(),
                    })
                }
            }
        }
        std::fs::write(&abs, content)
            .map_err(|e| ApiError::Io(format!("Failed to write {}: {}", file_path, e)))?;
        Ok(version_of(content.as_bytes()))
    }

    /// Create a directory (and any missing parents) within a project.
    pub fn create_dir(&self, project_path: &Path, dir_path: &str) -> Result<(), ApiError> {
        validate_path(dir_path)?;
        let abs = project_path.join(dir_path);
        if abs.exists() {
            return Err(ApiError::InvalidArgument(format!(
                "Path already exists: {}",
                dir_path
            )));
        }
        std::fs::create_dir_all(&abs)
            .map_err(|e| ApiError::Io(format!("Failed to create directory {}: {}", dir_path, e)))
    }

    /// Rename (move) a file within a project.
    pub fn rename(
        &self,
        project_path: &Path,
        old_path: &str,
        new_path: &str,
    ) -> Result<(), ApiError> {
        validate_path(old_path)?;
        validate_path(new_path)?;
        let abs_old = project_path.join(old_path);
        let abs_new = project_path.join(new_path);
        if !abs_old.exists() {
            return Err(ApiError::NotFound(format!("File not found: {}", old_path)));
        }
        if abs_new.exists() {
            return Err(ApiError::InvalidArgument(format!(
                "Destination already exists: {}",
                new_path
            )));
        }
        // Ensure parent directory exists for the target path.
        if let Some(parent) = abs_new.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ApiError::Io(format!("Failed to create directory: {}", e)))?;
        }
        std::fs::rename(&abs_old, &abs_new).map_err(|e| {
            ApiError::Io(format!(
                "Failed to rename {} → {}: {}",
                old_path, new_path, e
            ))
        })
    }

    /// Paste files/directories into a target directory inside the project.
    /// `op` is "copy" or "cut". Sources are absolute paths (from clipboard).
    /// Returns created project-relative paths and their source mappings.
    pub fn paste(
        &self,
        project_path: &Path,
        target: &str,
        op: &str,
        sources: &[String],
        collision_policy: &str,
        rename_map: &HashMap<String, String>,
    ) -> Result<Vec<FilePasteMapping>, ApiError> {
        let op = match op {
            "copy" => PasteOp::Copy,
            "cut" => PasteOp::Cut,
            _ => return Err(ApiError::InvalidArgument(format!("Invalid op: {}", op))),
        };
        let collision_policy = match collision_policy {
            "auto_rename" => CollisionPolicy::AutoRename,
            "replace" => CollisionPolicy::Replace,
            "skip" => CollisionPolicy::Skip,
            "rename" => CollisionPolicy::Rename,
            "error" => CollisionPolicy::Error,
            _ => {
                return Err(ApiError::InvalidArgument(format!(
                    "Invalid collision policy: {}",
                    collision_policy
                )))
            }
        };
        let abs_target_dir = target_dir(project_path, target)?;

        let mut mappings = Vec::new();

        for source in sources {
            let src_path = Path::new(source);
            let name = src_path.file_name().ok_or_else(|| {
                ApiError::InvalidArgument(format!("Invalid source path: {}", source))
            })?;

            // For explicit rename resolution, allow the caller to choose
            // a different basename for this source.
            let mut desired_dest = abs_target_dir.join(name);
            if collision_policy == CollisionPolicy::Rename {
                if let Some(rename_to) = rename_map.get(source) {
                    validate_basename(rename_to)?;
                    desired_dest = abs_target_dir.join(rename_to);
                }
            }

            let dest_path = resolve_destination(
                src_path,
                source,
                &desired_dest,
                collision_policy,
                rename_map,
            )?;
            let Some(dest_path) = dest_path else {
                continue;
            };

            if op == PasteOp::Cut && src_path == dest_path {
                continue;
            }

            if op == PasteOp::Cut {
                // Try fast rename; fall back to copy+delete if it fails
                // (e.g. cross-filesystem moves).
                if std::fs::rename(src_path, &dest_path).is_err() {
                    copy_recursive_bounded(src_path, &dest_path, 0)?;
                    remove_recursive(src_path)?;
                }
            } else {
                copy_recursive_bounded(src_path, &dest_path, 0)?;
            }

            // Compute project-relative path for the created item
            if let Ok(rel) = dest_path.strip_prefix(project_path) {
                let destination = rel.to_string_lossy().into_owned();
                mappings.push(FilePasteMapping {
                    source: source.clone(),
                    destination,
                });
            }
        }

        Ok(mappings)
    }

    /// Return collisions for a paste/drop operation before transfer.
    pub fn paste_collisions(
        &self,
        project_path: &Path,
        target: &str,
        sources: &[String],
    ) -> Result<Vec<FilePasteCollision>, ApiError> {
        let abs_target_dir = target_dir(project_path, target)?;

        let mut collisions = Vec::new();
        for source in sources {
            let src_path = Path::new(source);
            let name = src_path.file_name().ok_or_else(|| {
                ApiError::InvalidArgument(format!("Invalid source path: {}", source))
            })?;
            let dest_path = abs_target_dir.join(name);
            if !dest_path.exists() {
                continue;
            }

            let destination = match dest_path.strip_prefix(project_path) {
                Ok(rel) => rel.to_string_lossy().into_owned(),
                Err(_) => dest_path.to_string_lossy().into_owned(),
            };
            collisions.push(FilePasteCollision {
                source: source.clone(),
                destination,
            });
        }

        Ok(collisions)
    }

    /// Delete a file within a project.
    pub fn delete(&self, project_path: &Path, file_path: &str) -> Result<(), ApiError> {
        validate_path(file_path)?;
        let abs = project_path.join(file_path);
        if !abs.exists() {
            return Err(ApiError::NotFound(format!("File not found: {}", file_path)));
        }
        if abs.is_dir() {
            std::fs::remove_dir_all(&abs)
                .map_err(|e| ApiError::Io(format!("Failed to delete {}: {}", file_path, e)))
        } else {
            std::fs::remove_file(&abs)
                .map_err(|e| ApiError::Io(format!("Failed to delete {}: {}", file_path, e)))
        }
    }

    /// List one directory, the way the explorer renders it.
    ///
    /// Filesystem-first and lazy: a folder open reads only the root, and each
    /// expand reads exactly one more directory. There is no file cap, so no
    /// sibling can be silently dropped. Empty directories are listed like any
    /// other entry.
    ///
    /// `dir_path` is project-relative; "" is the project root.
    pub fn read_dir(
        &self,
        project_path: &Path,
        dir_path: &str,
        show_hidden: bool,
        generation: u64,
    ) -> Result<Vec<DirEntry>, ApiError> {
        validate_path(dir_path)?;
        let filter = self.filter(project_path, generation)?;
        read_dir_with_filter(project_path, dir_path, show_hidden, &filter)
    }

    /// Flat list of every searchable file path, for Quick Open and the sidebar
    /// filter. Prunes git-ignored paths (VS Code's `search.useIgnoreFiles`
    /// default) independently of the explorer's dim-vs-hide setting.
    pub fn list_paths(
        &self,
        project_path: &Path,
        show_hidden: bool,
        generation: u64,
    ) -> Result<PathList, ApiError> {
        let filter = self.filter(project_path, generation)?;
        Ok(list_paths_with_filter(
            project_path,
            show_hidden,
            &filter,
            MAX_SEARCH_PATHS,
        ))
    }

    /// Drop a closed project's compiled filter, whose per-directory gitignore
    /// cache would otherwise live for the rest of the process.
    pub fn evict(&self, project_path: &Path) {
        self.filters.lock().remove(project_path);
    }

    fn filter(
        &self,
        project_path: &Path,
        generation: u64,
    ) -> Result<Arc<ExplorerFilter>, ApiError> {
        let mut filters = self.filters.lock();
        if let Some((cached_generation, filter)) = filters.get(project_path) {
            if *cached_generation == generation {
                return Ok(Arc::clone(filter));
            }
        }

        let resolved = resolve_effective_settings_for_folder_path(Some(project_path))
            .map_err(|error| ApiError::Io(format!("Failed to resolve settings: {error}")))?;
        let filter = Arc::new(ExplorerFilter::build(
            project_path,
            &resolved.settings.explorer,
        )?);
        filters.insert(
            project_path.to_path_buf(),
            (generation, Arc::clone(&filter)),
        );
        Ok(filter)
    }
}

/// Pair the bytes' version with their text. Hashing the bytes rather than the
/// decoded string keeps the version exact for whatever is actually on disk.
fn file_content(bytes: Vec<u8>, file_path: &str) -> Result<FileContent, ApiError> {
    let version = version_of(&bytes);
    let content = String::from_utf8(bytes)
        .map_err(|error| ApiError::Io(format!("Failed to read {file_path}: {error}")))?;
    Ok(FileContent { content, version })
}

/// Hex SHA-256 of a file's bytes.
fn version_of(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Entry survivors of one directory, before compaction.
struct RawEntry {
    name: String,
    /// Lowercased `name`, kept so sorting a large listing allocates once per
    /// entry instead of twice per comparison.
    sort_name: String,
    path: String,
    is_dir: bool,
    ignored: bool,
    excluded: bool,
}

fn read_dir_with_filter(
    project_path: &Path,
    dir_path: &str,
    show_hidden: bool,
    filter: &ExplorerFilter,
) -> Result<Vec<DirEntry>, ApiError> {
    // Children of an ignored directory are ignored too, and git never descends
    // to say so a second time.
    let parent_ignored = filter.is_ignored(dir_path, true);
    let mut entries = read_entries(
        project_path,
        dir_path,
        show_hidden,
        parent_ignored,
        filter,
        usize::MAX,
    )?;
    // Directories first, then case-insensitive alphabetical.
    entries.sort_by(|a, b| {
        (!a.is_dir, &a.sort_name, &a.name).cmp(&(!b.is_dir, &b.sort_name, &b.name))
    });

    Ok(entries
        .into_iter()
        .map(|entry| {
            if entry.is_dir && filter.compact_folders {
                compact(project_path, entry, show_hidden, filter)
            } else {
                DirEntry {
                    name: entry.name,
                    path: entry.path,
                    is_dir: entry.is_dir,
                    ignored: entry.ignored,
                    excluded: entry.excluded,
                    hops: Vec::new(),
                }
            }
        })
        .collect())
}

/// The entries of `dir_path` the explorer would render, at most `limit` of
/// them. Compaction only ever asks "exactly one child?", so it stops at two
/// rather than enumerating a `node_modules`-sized directory to find out.
fn read_entries(
    project_path: &Path,
    dir_path: &str,
    show_hidden: bool,
    parent_ignored: bool,
    filter: &ExplorerFilter,
    limit: usize,
) -> Result<Vec<RawEntry>, ApiError> {
    let abs = project_path.join(dir_path);
    // Every entry of this directory answers to the same ancestor `.gitignore`
    // chain, so it is resolved once rather than per entry.
    let chain = filter.ignore_chain(dir_path);
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&abs)
        .map_err(|e| ApiError::Io(format!("Cannot read {}: {}", abs.display(), e)))?
    {
        let entry = entry.map_err(|e| ApiError::Io(e.to_string()))?;
        if let Some(survivor) = survivor(
            &entry,
            dir_path,
            show_hidden,
            parent_ignored,
            filter,
            &chain,
        )? {
            out.push(survivor);
            if out.len() >= limit {
                break;
            }
        }
    }
    Ok(out)
}

/// One directory entry as the explorer sees it, or `None` when the exclude
/// globs or git's ignore rules hide it.
fn survivor(
    entry: &std::fs::DirEntry,
    dir_path: &str,
    show_hidden: bool,
    parent_ignored: bool,
    filter: &ExplorerFilter,
    chain: &IgnoreChain<'_>,
) -> Result<Option<RawEntry>, ApiError> {
    let name = entry.file_name().to_string_lossy().into_owned();
    let file_type = entry.file_type().map_err(|e| ApiError::Io(e.to_string()))?;
    // `file_type()` is lstat-based — for symlinks we need stat (`metadata`) to
    // see whether the link points at a directory.
    let is_dir = if file_type.is_symlink() {
        std::fs::metadata(entry.path())
            .map(|m| m.is_dir())
            .unwrap_or(false)
    } else {
        file_type.is_dir()
    };

    let path = if dir_path.is_empty() {
        name.clone()
    } else {
        format!("{dir_path}/{name}")
    };

    let excluded = filter.is_excluded(&path);
    if excluded && !show_hidden {
        return Ok(None);
    }
    let ignored = parent_ignored || chain.is_ignored(&path, is_dir);
    if ignored && filter.exclude_gitignore && !show_hidden {
        return Ok(None);
    }

    Ok(Some(RawEntry {
        sort_name: name.to_lowercase(),
        name,
        path,
        is_dir,
        ignored,
        excluded,
    }))
}

/// Collapse a chain of single-child directories into one row, matching VS
/// Code's `explorer.compactFolders`. A directory that can't be listed stops the
/// descent and is reported uncompacted.
fn compact(
    project_path: &Path,
    entry: RawEntry,
    show_hidden: bool,
    filter: &ExplorerFilter,
) -> DirEntry {
    let mut row = DirEntry {
        name: entry.name,
        path: entry.path,
        is_dir: true,
        ignored: entry.ignored,
        excluded: entry.excluded,
        hops: Vec::new(),
    };

    for _ in 0..MAX_COMPACT_HOPS {
        let Ok(mut children) =
            read_entries(project_path, &row.path, show_hidden, row.ignored, filter, 2)
        else {
            break;
        };
        if children.len() != 1 || !children[0].is_dir {
            break;
        }
        let only = children.remove(0);
        row.name = format!("{}/{}", row.name, only.name);
        row.hops.push(std::mem::replace(&mut row.path, only.path));
        row.ignored = only.ignored;
        row.excluded = only.excluded;
    }

    row
}

fn list_paths_with_filter(
    project_path: &Path,
    show_hidden: bool,
    filter: &Arc<ExplorerFilter>,
    limit: usize,
) -> PathList {
    let root = project_path.to_path_buf();
    let prune_root = root.clone();
    let prune_filter = Arc::clone(filter);
    let mut walker = ignore::WalkBuilder::new(project_path);
    walker
        // Dotfiles are legitimate project files; `.git` is pruned below.
        .hidden(false)
        .parents(true)
        .follow_links(true)
        .max_depth(Some(MAX_DEPTH))
        .git_ignore(!show_hidden)
        .git_global(!show_hidden)
        .git_exclude(!show_hidden)
        .require_git(true)
        // Pruning here (rather than filtering results) is what keeps excluded
        // trees from being walked at all.
        .filter_entry(move |entry| {
            let Ok(rel) = entry.path().strip_prefix(&prune_root) else {
                return true;
            };
            // Pruning `.git` at its own level means no descendant is ever
            // visited, so only this entry's own name needs checking.
            if rel.file_name().is_some_and(|name| name == ".git") {
                return false;
            }
            let rel = rel.to_string_lossy().replace('\\', "/");
            show_hidden || rel.is_empty() || !prune_filter.is_excluded(&rel)
        });

    let mut paths = Vec::new();
    let mut truncated = false;
    for entry in walker.build() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::debug!(%error, "skipping unreadable path during search walk");
                continue;
            }
        };
        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            continue;
        }
        if paths.len() >= limit {
            truncated = true;
            break;
        }
        let Ok(rel) = entry.path().strip_prefix(&root) else {
            continue;
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        paths.push(rel.to_string_lossy().replace('\\', "/"));
    }

    paths.sort();
    PathList { paths, truncated }
}

// ── Paste helpers ─────────────────────────────────────────────────

/// Reject lexical paths that can escape the project root.
fn validate_path(file_path: &str) -> Result<(), ApiError> {
    if Path::new(file_path).components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(ApiError::InvalidArgument(format!(
            "Invalid file path: {}",
            file_path
        )));
    }
    Ok(())
}

fn target_dir(project: &Path, target: &str) -> Result<PathBuf, ApiError> {
    validate_path(target)?;
    let path = project.join(target);
    if !path.exists() {
        return Err(ApiError::NotFound(format!(
            "Target directory not found: {}",
            target
        )));
    }
    if !path.is_dir() {
        return Err(ApiError::InvalidArgument(format!(
            "Target is not a directory: {}",
            target
        )));
    }
    Ok(path)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PasteOp {
    Copy,
    Cut,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CollisionPolicy {
    AutoRename,
    Replace,
    Skip,
    Rename,
    Error,
}

fn resolve_destination(
    src_path: &Path,
    source_key: &str,
    desired_dest: &Path,
    collision_policy: CollisionPolicy,
    rename_map: &HashMap<String, String>,
) -> Result<Option<PathBuf>, ApiError> {
    if !desired_dest.exists() {
        return Ok(Some(desired_dest.to_path_buf()));
    }

    match collision_policy {
        CollisionPolicy::AutoRename => Ok(Some(unique_path(desired_dest))),
        CollisionPolicy::Replace => {
            if desired_dest == src_path {
                return Ok(Some(desired_dest.to_path_buf()));
            }
            if src_path.starts_with(desired_dest) {
                return Err(ApiError::InvalidArgument(format!(
                    "Cannot replace destination containing source: {}",
                    desired_dest.display()
                )));
            }
            // Resolve parent aliases, but preserve the final entry's symlink identity.
            let normalize_entry = |path: &Path| -> Result<PathBuf, ApiError> {
                let name = path.file_name().ok_or_else(|| {
                    ApiError::InvalidArgument(format!("Invalid entry path: {}", path.display()))
                })?;
                let parent = path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                std::fs::canonicalize(parent)
                    .map(|parent| parent.join(name))
                    .map_err(|error| {
                        ApiError::Io(format!(
                            "Cannot resolve parent of {}: {}",
                            path.display(),
                            error
                        ))
                    })
            };
            let normalized_source = normalize_entry(src_path)?;
            let normalized_destination = normalize_entry(desired_dest)?;
            if normalized_source == normalized_destination {
                return Ok(Some(desired_dest.to_path_buf()));
            }
            if normalized_source.starts_with(&normalized_destination) {
                return Err(ApiError::InvalidArgument(format!(
                    "Cannot replace destination containing source: {}",
                    desired_dest.display()
                )));
            }
            remove_recursive(desired_dest)?;
            Ok(Some(desired_dest.to_path_buf()))
        }
        CollisionPolicy::Skip => Ok(None),
        CollisionPolicy::Rename => {
            if !rename_map.contains_key(source_key) {
                return Err(ApiError::InvalidArgument(format!(
                    "Rename policy requires rename_map entry for source: {}",
                    source_key
                )));
            }
            Err(ApiError::InvalidArgument(format!(
                "Destination already exists: {}",
                desired_dest.display()
            )))
        }
        CollisionPolicy::Error => Err(ApiError::InvalidArgument(format!(
            "Destination already exists: {}",
            desired_dest.display()
        ))),
    }
}

fn validate_basename(value: &str) -> Result<(), ApiError> {
    if value.trim().is_empty() {
        return Err(ApiError::InvalidArgument(
            "Rename value cannot be empty".into(),
        ));
    }
    if value == "." || value == ".." || value.contains('/') || value.contains('\\') {
        return Err(ApiError::InvalidArgument(format!(
            "Invalid rename value: {}",
            value
        )));
    }
    Ok(())
}

/// Return a non-colliding path by appending " (copy)", " (copy 2)", etc.
fn unique_path(desired: &Path) -> std::path::PathBuf {
    let parent = desired.parent().unwrap_or_else(|| Path::new(""));
    let stem = desired
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = desired
        .extension()
        .map(|s| s.to_string_lossy().into_owned());

    for i in 1..1000 {
        let name = if i == 1 {
            match &ext {
                Some(e) => format!("{} (copy).{}", stem, e),
                None => format!("{} (copy)", stem),
            }
        } else {
            match &ext {
                Some(e) => format!("{} (copy {}).{}", stem, i, e),
                None => format!("{} (copy {})", stem, i),
            }
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    desired.to_path_buf()
}

/// Recursively copy a file or directory. Capped at `MAX_DEPTH` to prevent
/// runaway recursion on symlink loops or pathological bind mounts.
fn copy_recursive_bounded(src: &Path, dest: &Path, depth: usize) -> Result<(), ApiError> {
    if depth > MAX_DEPTH {
        return Err(ApiError::Io(format!(
            "Copy aborted: max depth {} exceeded at {}",
            MAX_DEPTH,
            src.display()
        )));
    }
    let metadata = std::fs::symlink_metadata(src)
        .map_err(|e| ApiError::Io(format!("Cannot stat {}: {}", src.display(), e)))?;

    if metadata.is_dir() {
        std::fs::create_dir_all(dest)
            .map_err(|e| ApiError::Io(format!("Cannot create {}: {}", dest.display(), e)))?;
        let entries = std::fs::read_dir(src)
            .map_err(|e| ApiError::Io(format!("Cannot read {}: {}", src.display(), e)))?;
        for entry in entries {
            let entry = entry.map_err(|e| ApiError::Io(e.to_string()))?;
            let child_src = entry.path();
            let child_dest = dest.join(entry.file_name());
            copy_recursive_bounded(&child_src, &child_dest, depth + 1)?;
        }
    } else {
        std::fs::copy(src, dest).map_err(|e| {
            ApiError::Io(format!(
                "Cannot copy {} -> {}: {}",
                src.display(),
                dest.display(),
                e
            ))
        })?;
    }
    Ok(())
}

/// Recursively remove a file or directory.
fn remove_recursive(path: &Path) -> Result<(), ApiError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| ApiError::Io(format!("Cannot stat {}: {}", path.display(), e)))?;
    if metadata.is_dir() {
        std::fs::remove_dir_all(path)
            .map_err(|e| ApiError::Io(format!("Cannot remove {}: {}", path.display(), e)))
    } else {
        std::fs::remove_file(path)
            .map_err(|e| ApiError::Io(format!("Cannot remove {}: {}", path.display(), e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn unique_test_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "sworm-files-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    fn touch(path: PathBuf) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(path, "").expect("write file");
    }

    /// Filters are built directly so tests never depend on an on-disk
    /// `settings.jsonc`.
    fn filter(root: &Path, exclude_gitignore: bool, compact_folders: bool) -> Arc<ExplorerFilter> {
        let settings = ExplorerSettings {
            exclude: BTreeMap::from([("**/.git".to_string(), true)]),
            exclude_gitignore,
            compact_folders,
        };
        Arc::new(ExplorerFilter::build(root, &settings).expect("build filter"))
    }

    fn names(entries: &[DirEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.name.as_str()).collect()
    }

    #[test]
    fn paste_replace_refuses_destination_containing_source() {
        let service = FileService::new();
        let mut failures = Vec::new();
        for op in ["cut", "copy"] {
            let mut cases = vec!["nested", "parent-traversal"];
            #[cfg(unix)]
            cases.extend(["parent-alias", "destination-symlink"]);
            for case in cases {
                let dir = unique_test_dir(&format!("paste-replace-{op}-{case}"));
                let destination = dir.join("item");
                let source = match case {
                    "nested" => {
                        std::fs::create_dir(&destination).unwrap();
                        destination.join("item")
                    }
                    "parent-traversal" => {
                        std::fs::create_dir(&destination).unwrap();
                        std::fs::create_dir(dir.join("other")).unwrap();
                        dir.join("other/../item/item")
                    }
                    #[cfg(unix)]
                    "parent-alias" => {
                        std::fs::create_dir(&destination).unwrap();
                        std::os::unix::fs::symlink(&destination, dir.join("alias")).unwrap();
                        dir.join("alias/item")
                    }
                    #[cfg(unix)]
                    "destination-symlink" => {
                        let real = dir.join("real");
                        std::fs::create_dir(&real).unwrap();
                        std::os::unix::fs::symlink(&real, &destination).unwrap();
                        destination.join("item")
                    }
                    _ => unreachable!(),
                };
                std::fs::write(&source, "source content").unwrap();
                let source_key = source.to_string_lossy().into_owned();
                let result = service.paste(
                    &dir,
                    "",
                    op,
                    &[source_key],
                    "replace",
                    &HashMap::new(),
                );
                if !matches!(result, Err(ApiError::InvalidArgument(_))) {
                    failures.push(format!(
                        "{op}/{case}: expected InvalidArgument, got {result:?}"
                    ));
                }
                let contents = std::fs::read_to_string(&source);
                if contents.as_deref().ok() != Some("source content") {
                    failures.push(format!("{op}/{case}: source lost: {contents:?}"));
                }
                std::fs::remove_dir_all(dir).unwrap();
            }

            let dir = unique_test_dir(&format!("paste-replace-{op}-sibling"));
            std::fs::create_dir(dir.join("from")).unwrap();
            std::fs::create_dir(dir.join("to")).unwrap();
            let source = dir.join("from/item");
            std::fs::write(&source, "source content").unwrap();
            std::fs::write(dir.join("to/item"), "old content").unwrap();
            let mappings = service
                .paste(
                    &dir,
                    "to",
                    op,
                    &[source.to_string_lossy().into_owned()],
                    "replace",
                    &HashMap::new(),
                )
                .unwrap();
            assert_eq!(mappings.len(), 1);
            assert_eq!(mappings[0].destination, "to/item");
            assert_eq!(
                std::fs::read_to_string(dir.join("to/item")).unwrap(),
                "source content"
            );
            assert_eq!(source.exists(), op == "copy");
            if op == "copy" {
                assert_eq!(std::fs::read_to_string(&source).unwrap(), "source content");
            }
            std::fs::remove_dir_all(dir).unwrap();

            // A missing source parent must not erase an existing destination.
            let dir = unique_test_dir(&format!("paste-replace-{op}-missing-parent"));
            let destination = dir.join("item");
            std::fs::write(&destination, "old content").unwrap();
            assert!(matches!(
                service.paste(
                    &dir,
                    "",
                    op,
                    &[dir.join("missing/item").to_string_lossy().into_owned()],
                    "replace",
                    &HashMap::new(),
                ),
                Err(ApiError::Io(_))
            ));
            assert_eq!(
                std::fs::read_to_string(&destination).unwrap(),
                "old content"
            );
            std::fs::remove_dir_all(dir).unwrap();

            #[cfg(unix)]
            {
                // Replacing the final symlink is safe for a source addressed independently.
                let dir = unique_test_dir(&format!("paste-replace-{op}-safe-symlink"));
                let real = dir.join("real");
                std::fs::create_dir(&real).unwrap();
                let source = real.join("item");
                std::fs::write(&source, "source content").unwrap();
                let destination = dir.join("item");
                std::os::unix::fs::symlink(&real, &destination).unwrap();
                let mappings = service
                    .paste(
                        &dir,
                        "",
                        op,
                        &[source.to_string_lossy().into_owned()],
                        "replace",
                        &HashMap::new(),
                    )
                    .unwrap();
                assert_eq!(mappings.len(), 1);
                assert_eq!(mappings[0].destination, "item");
                assert!(!std::fs::symlink_metadata(&destination).unwrap().is_symlink());
                assert_eq!(
                    std::fs::read_to_string(&destination).unwrap(),
                    "source content"
                );
                assert!(real.is_dir());
                assert_eq!(source.exists(), op == "copy");
                if op == "copy" {
                    assert_eq!(std::fs::read_to_string(&source).unwrap(), "source content");
                }
                std::fs::remove_dir_all(dir).unwrap();
            }
        }

        let dir = unique_test_dir("paste-replace-same-entry");
        std::fs::create_dir(dir.join("folder")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.join("folder"), dir.join("alias")).unwrap();
        let destination = dir.join("folder/item");
        let mut sources = vec![destination.clone(), dir.join("folder/../folder/item")];
        #[cfg(unix)]
        sources.push(dir.join("alias/item"));
        for source in sources {
            std::fs::write(&destination, "source content").unwrap();
            let result = resolve_destination(
                &source,
                &source.to_string_lossy(),
                &destination,
                CollisionPolicy::Replace,
                &HashMap::new(),
            )
            .unwrap();
            assert_eq!(result, Some(destination.clone()));
            let contents = std::fs::read_to_string(&destination);
            if contents.as_deref().ok() != Some("source content") {
                failures.push(format!(
                    "same-entry/{}: source lost: {contents:?}",
                    source.display()
                ));
            }
        }
        std::fs::write(&destination, "source content").unwrap();
        assert!(
            service
                .paste(
                    &dir,
                    "folder",
                    "cut",
                    &[destination.to_string_lossy().into_owned()],
                    "replace",
                    &HashMap::new(),
                )
                .unwrap()
                .is_empty()
        );
        assert_eq!(std::fs::read_to_string(&destination).unwrap(), "source content");
        std::fs::remove_dir_all(dir).unwrap();
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn paste_rename_never_overwrites_existing_destination() {
        let dir = unique_test_dir("paste-rename");
        let source = dir.join("source.txt").to_string_lossy().into_owned();
        std::fs::write(&source, "source content").unwrap();
        std::fs::write(dir.join("other.txt"), "other content").unwrap();
        let service = FileService::new();
        for name in ["source.txt", "other.txt"] {
            let rename_map = HashMap::from([(source.clone(), name.to_string())]);
            assert!(matches!(
                service.paste(
                    &dir,
                    "",
                    "copy",
                    std::slice::from_ref(&source),
                    "rename",
                    &rename_map
                ),
                Err(ApiError::InvalidArgument(_))
            ));
            assert_eq!(std::fs::read_to_string(&source).unwrap(), "source content");
            assert_eq!(
                std::fs::read_to_string(dir.join("other.txt")).unwrap(),
                "other content"
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn streaming_reads_to_verified_eof_with_one_buffer() {
        use sworm_protocol::rpc::MAX_FILE_CHUNK_BYTES;
        let dir = unique_test_dir("stream-eof");
        let content: Vec<u8> = (0..MAX_FILE_CHUNK_BYTES + 7).map(|i| i as u8).collect();
        std::fs::write(dir.join("a.txt"), &content).unwrap();
        let service = FileService::new();
        let stat = service.stat(&dir, "a.txt").unwrap();
        let mut reader = service
            .open_read_stream(&dir, "a.txt", &stat.version)
            .unwrap();
        assert_eq!(reader.size(), content.len() as u64);
        let mut chunk = Vec::with_capacity(MAX_FILE_CHUNK_BYTES);
        let mut assembled = Vec::new();
        loop {
            chunk.clear();
            if reader.read_chunk(&mut chunk).unwrap() == 0 {
                break;
            }
            assert!(chunk.len() <= MAX_FILE_CHUNK_BYTES);
            assembled.extend_from_slice(&chunk);
        }
        assert_eq!(assembled, content);
        assert_eq!(reader.version(), version_of(&content));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn streaming_rejects_stale_identity_and_mid_read_replacement() {
        use sworm_protocol::rpc::MAX_FILE_CHUNK_BYTES;
        let dir = unique_test_dir("stream-mutation");
        let path = dir.join("a.txt");
        std::fs::write(&path, vec![b'a'; 2 * MAX_FILE_CHUNK_BYTES]).unwrap();
        let service = FileService::new();
        let stat = service.stat(&dir, "a.txt").unwrap();
        let mut reader = service
            .open_read_stream(&dir, "a.txt", &stat.version)
            .unwrap();
        let mut chunk = Vec::new();
        assert_eq!(reader.read_chunk(&mut chunk).unwrap(), MAX_FILE_CHUNK_BYTES);
        assert_eq!(chunk, vec![b'a'; MAX_FILE_CHUNK_BYTES]);
        std::fs::write(dir.join("replacement"), vec![b'b'; stat.size as usize]).unwrap();
        std::fs::rename(dir.join("replacement"), &path).unwrap();
        assert!(matches!(
            reader.read_chunk(&mut chunk),
            Err(ApiError::Conflict { .. })
        ));
        assert!(matches!(
            service.open_read_stream(&dir, "a.txt", &stat.version),
            Err(ApiError::Conflict { .. })
        ));
        let stat = service.stat(&dir, "a.txt").unwrap();
        let mut reader = service
            .open_read_stream(&dir, "a.txt", &stat.version)
            .unwrap();
        reader.read_chunk(&mut Vec::new()).unwrap();
        OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(1)
            .unwrap();
        assert!(matches!(
            reader.read_chunk(&mut Vec::new()),
            Err(ApiError::Conflict { .. })
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn whole_file_read_refuses_oversized_and_non_regular_files() {
        use sworm_protocol::rpc::MAX_WHOLE_FILE_BYTES;
        let dir = unique_test_dir("read-limits");
        let limit = MAX_WHOLE_FILE_BYTES as u64;
        File::create(dir.join("at-limit"))
            .unwrap()
            .set_len(limit)
            .unwrap();
        File::create(dir.join("over-limit"))
            .unwrap()
            .set_len(limit + 1)
            .unwrap();
        let service = FileService::new();
        assert_eq!(
            service.read(&dir, "at-limit").unwrap().content.len() as u64,
            limit
        );
        assert!(matches!(
            service.read(&dir, "over-limit"),
            Err(ApiError::TooLarge { size, limit: reported })
                if size == limit + 1 && reported == limit
        ));
        // A FIFO with no writer would block a plain open forever.
        let fifo =
            std::ffi::CString::new(dir.join("fifo").into_os_string().into_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(matches!(
            service.read(&dir, "fifo"),
            Err(ApiError::InvalidArgument(message)) if message.contains("not a regular file")
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn file_write_conflict_detects_external_change() {
        let dir = unique_test_dir("write-conflict");
        std::fs::write(dir.join("a.txt"), "original\n").expect("seed file");
        let service = FileService::new();

        let opened = service.read(&dir, "a.txt").expect("read");
        assert_eq!(opened.content, "original\n");

        // Someone else (an agent, another editor) rewrites the file.
        std::fs::write(dir.join("a.txt"), "theirs\n").expect("external write");
        let stale = service
            .write(&dir, "a.txt", "mine\n", Some(&opened.version))
            .expect_err("stale write must be refused");
        let current_version = match stale {
            ApiError::Conflict { current_version } => current_version,
            other => panic!("expected Conflict, got {other:?}"),
        };
        assert_eq!(
            current_version,
            service.read(&dir, "a.txt").expect("re-read").version
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).expect("unchanged"),
            "theirs\n"
        );

        // Explicit overwrite skips the check, and hands back the version of
        // what it wrote so the next save doesn't conflict with this one.
        let written = service
            .write(&dir, "a.txt", "mine\n", None)
            .expect("overwrite");
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).expect("overwritten"),
            "mine\n"
        );
        assert_eq!(
            written,
            service.read(&dir, "a.txt").expect("re-read").version
        );

        service
            .write(&dir, "a.txt", "mine again\n", Some(&written))
            .expect("the returned version writes through");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_write_refuses_to_resurrect_a_deleted_file() {
        let dir = unique_test_dir("write-deleted");
        std::fs::write(dir.join("a.txt"), "original\n").expect("seed file");
        let service = FileService::new();
        let opened = service.read(&dir, "a.txt").expect("read");

        std::fs::remove_file(dir.join("a.txt")).expect("external delete");
        let refused = service
            .write(&dir, "a.txt", "mine\n", Some(&opened.version))
            .expect_err("a stale save must not undo the deletion");
        assert!(
            matches!(&refused, ApiError::Deleted { path } if path == "a.txt"),
            "expected Deleted, got {refused:?}"
        );
        assert!(!dir.join("a.txt").exists(), "the file stayed deleted");

        // Recreating it is a deliberate act: an explicit unconditional write.
        let written = service
            .write(&dir, "a.txt", "mine\n", None)
            .expect("explicit overwrite recreates");
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).expect("recreated"),
            "mine\n"
        );
        assert_eq!(
            written,
            service.read(&dir, "a.txt").expect("re-read").version
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_write_creates_a_new_file_but_not_a_missing_parent() {
        let dir = unique_test_dir("write-new");
        let service = FileService::new();

        service
            .write(&dir, "new.txt", "hello\n", None)
            .expect("a brand new file needs no version");
        service
            .write(&dir, "missing/new.txt", "hello\n", None)
            .expect_err("a missing parent directory is not created implicitly");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_dir_lists_empty_directories() {
        let dir = unique_test_dir("empty-dirs");
        std::fs::create_dir_all(dir.join("empty")).expect("empty dir");
        touch(dir.join("a.txt"));

        let entries =
            read_dir_with_filter(&dir, "", false, &filter(&dir, false, false)).expect("read dir");

        assert_eq!(names(&entries), vec!["empty", "a.txt"]);
        assert!(entries[0].is_dir);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_dir_hides_git_by_default_and_reveals_it_with_show_hidden() {
        let dir = unique_test_dir("git-row");
        std::fs::create_dir_all(dir.join(".git")).expect("fake git dir");
        touch(dir.join("src/main.rs"));

        let filter = filter(&dir, false, false);
        let hidden = read_dir_with_filter(&dir, "", false, &filter).expect("read dir");
        assert_eq!(names(&hidden), vec!["src"]);

        let shown = read_dir_with_filter(&dir, "", true, &filter).expect("read dir");
        assert_eq!(names(&shown), vec![".git", "src"]);
        assert!(shown[0].excluded);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_dir_compacts_single_child_chains() {
        let dir = unique_test_dir("compact");
        touch(dir.join("src/lib/utils/a.ts"));

        let compacted =
            read_dir_with_filter(&dir, "", false, &filter(&dir, false, true)).expect("read dir");
        assert_eq!(names(&compacted), vec!["src/lib/utils"]);
        assert_eq!(compacted[0].path, "src/lib/utils");
        // The swallowed directories: watching these is the only way a write
        // inside them can split the row back apart.
        assert_eq!(compacted[0].hops, vec!["src", "src/lib"]);

        touch(dir.join("src/other.ts"));
        let split =
            read_dir_with_filter(&dir, "", false, &filter(&dir, false, true)).expect("read dir");
        assert_eq!(names(&split), vec!["src"]);
        assert_eq!(split[0].path, "src");
        assert!(split[0].hops.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_dir_marks_gitignored_entries_without_hiding_them() {
        let dir = unique_test_dir("ignored-dim");
        std::fs::create_dir_all(dir.join(".git")).expect("fake git dir");
        std::fs::write(dir.join(".gitignore"), "gen/\n").expect("ignore file");
        touch(dir.join("gen/out.js"));
        touch(dir.join("gen/deep/inner.js"));
        touch(dir.join("src/main.rs"));

        let filter = filter(&dir, false, false);
        let root = read_dir_with_filter(&dir, "", false, &filter).expect("read dir");
        assert_eq!(names(&root), vec!["gen", "src", ".gitignore"]);
        let gen = root
            .iter()
            .find(|entry| entry.name == "gen")
            .expect("gen row");
        assert!(gen.ignored);
        assert!(
            !root
                .iter()
                .find(|entry| entry.name == "src")
                .expect("src row")
                .ignored
        );

        // Children inherit the parent's ignored state; git does not repeat itself.
        let children = read_dir_with_filter(&dir, "gen", false, &filter).expect("read dir");
        assert!(children.iter().all(|entry| entry.ignored));

        // Grandchildren of an ignored directory remain ignored too.
        let grandchildren =
            read_dir_with_filter(&dir, "gen/deep", false, &filter).expect("read dir");
        assert!(grandchildren[0].ignored);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_dir_hides_gitignored_entries_when_configured() {
        let dir = unique_test_dir("ignored-hide");
        std::fs::create_dir_all(dir.join(".git")).expect("fake git dir");
        std::fs::write(dir.join(".gitignore"), "gen/\n").expect("ignore file");
        touch(dir.join("gen/out.js"));
        touch(dir.join("src/main.rs"));

        let filter = filter(&dir, true, false);
        let hidden = read_dir_with_filter(&dir, "", false, &filter).expect("read dir");
        assert_eq!(names(&hidden), vec!["src", ".gitignore"]);

        let shown = read_dir_with_filter(&dir, "", true, &filter).expect("read dir");
        assert_eq!(names(&shown), vec![".git", "gen", "src", ".gitignore"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_paths_prunes_git_and_gitignored_paths() {
        let dir = unique_test_dir("list-paths");
        std::fs::create_dir_all(dir.join(".git")).expect("fake git dir");
        touch(dir.join(".git/objects/deadbeef"));
        std::fs::write(dir.join(".gitignore"), "gen/\n").expect("ignore file");
        touch(dir.join("gen/out.js"));
        touch(dir.join("src/main.rs"));

        let filter = filter(&dir, false, false);
        let listed = list_paths_with_filter(&dir, false, &filter, MAX_SEARCH_PATHS);
        assert_eq!(listed.paths, vec![".gitignore", "src/main.rs"]);
        assert!(!listed.truncated);

        // show_hidden keeps ignored paths searchable but never walks `.git`.
        let all = list_paths_with_filter(&dir, true, &filter, MAX_SEARCH_PATHS);
        assert_eq!(all.paths, vec![".gitignore", "gen/out.js", "src/main.rs"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_paths_flags_truncation() {
        let dir = unique_test_dir("truncation");
        for index in 0..3 {
            touch(dir.join(format!("file-{index}.txt")));
        }

        let filter = filter(&dir, false, false);
        let mut listed = list_paths_with_filter(&dir, false, &filter, MAX_SEARCH_PATHS);
        assert!(!listed.truncated);

        listed = list_paths_with_filter(&dir, false, &filter, 2);
        assert!(listed.truncated);
        assert_eq!(listed.paths.len(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }
}
