//! Git worktree transactions. Destructive callers must first hold Host admission
//! exclusively, refuse remaining folder owners, and drain affected Sworm children.

use super::{git_command, git_output, read_operation, run_git_mutate, GitService};
use crate::errors::ApiError;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Weak};
use sworm_protocol::git::{
    GitWorktree, GitWorktreeCheckout, GitWorktreeList, GitWorktreeOp, GitWorktreeResult,
};

mod copy;

#[cfg(test)]
mod tests;

fn utf8(bytes: &[u8], what: &str) -> Result<String, ApiError> {
    std::str::from_utf8(bytes).map(str::to_owned).map_err(|_| {
        ApiError::InvalidArgument(format!("{what} is not UTF-8; worktree paths must be UTF-8"))
    })
}

fn path_string(path: &Path) -> Result<String, ApiError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| ApiError::InvalidArgument("Worktree path is not UTF-8".into()))
}

fn read_line(path: &Path, args: &[&str], action: &str) -> Result<String, ApiError> {
    super::git_output_line(path, args, action).map_err(ApiError::Internal)
}

/// Resolve existing ancestors without accepting traversal in the missing suffix.
fn absolute_path(path: &Path) -> Result<PathBuf, ApiError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(ApiError::InvalidArgument(
            "Worktree paths must be absolute and contain no '..' components".into(),
        ));
    }
    let mut cursor = path.to_path_buf();
    let mut missing = Vec::new();
    loop {
        match std::fs::canonicalize(&cursor) {
            Ok(mut anchor) => {
                for part in missing.iter().rev() {
                    anchor.push(part);
                }
                path_string(&anchor)?;
                return Ok(anchor);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // A dangling link is occupied, not a missing normal component.
                match std::fs::symlink_metadata(&cursor) {
                    Ok(_) => {
                        return Err(ApiError::InvalidArgument(format!(
                            "Path is a dangling symlink: {}",
                            cursor.display()
                        )))
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(ApiError::Io(format!(
                            "Cannot inspect {}: {error}",
                            cursor.display()
                        )))
                    }
                }
                missing.push(
                    cursor
                        .file_name()
                        .ok_or_else(|| {
                            ApiError::InvalidArgument("Invalid absolute worktree path".into())
                        })?
                        .to_os_string(),
                );
                cursor = cursor
                    .parent()
                    .ok_or_else(|| {
                        ApiError::InvalidArgument("Invalid absolute worktree path".into())
                    })?
                    .to_path_buf();
            }
            Err(error) => {
                return Err(ApiError::Io(format!(
                    "Cannot resolve {}: {error}",
                    cursor.display()
                )))
            }
        }
    }
}

fn inventory_path(path: &Path) -> Result<PathBuf, ApiError> {
    match std::fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // Stale registrations preserve Git's lexical absolute path.
            if !path.is_absolute() {
                return Err(ApiError::Internal(
                    "Git returned a relative worktree path".into(),
                ));
            }
            Ok(path.to_path_buf())
        }
        Err(error) => Err(ApiError::Io(format!(
            "Cannot inspect worktree {}: {error}",
            path.display()
        ))),
    }
}

fn parse_inventory(bytes: &[u8], current: Option<&Path>) -> Result<Vec<GitWorktree>, ApiError> {
    let mut rows = Vec::new();
    let mut row: Option<GitWorktree> = None;
    for field in bytes.split(|byte| *byte == 0) {
        if field.is_empty() {
            if let Some(mut entry) = row.take() {
                entry.is_main = rows.is_empty();
                rows.push(entry);
            }
            continue;
        }
        if let Some(raw) = field.strip_prefix(b"worktree ") {
            if row.is_some() {
                return Err(ApiError::Internal(
                    "Malformed Git worktree inventory boundary".into(),
                ));
            }
            let path = inventory_path(Path::new(&utf8(raw, "Worktree path")?))?;
            row = Some(GitWorktree {
                is_current: current == Some(path.as_path()),
                path: path_string(&path)?,
                head: None,
                branch: None,
                is_main: false,
                bare: false,
                locked: None,
                prunable: None,
            });
            continue;
        }
        let entry = row
            .as_mut()
            .ok_or_else(|| ApiError::Internal("Malformed Git worktree inventory record".into()))?;
        if let Some(head) = field.strip_prefix(b"HEAD ") {
            let head = utf8(head, "Worktree HEAD")?;
            if !head.bytes().all(|byte| byte == b'0') {
                entry.head = Some(head);
            }
        } else if let Some(branch) = field.strip_prefix(b"branch refs/heads/") {
            entry.branch = Some(utf8(branch, "Worktree branch")?);
        } else if field == b"bare" {
            entry.bare = true;
        } else if field == b"locked" {
            entry.locked = Some(String::new());
        } else if let Some(reason) = field.strip_prefix(b"locked ") {
            entry.locked = Some(utf8(reason, "Worktree lock reason")?);
        } else if field == b"prunable" {
            entry.prunable = Some(String::new());
        } else if let Some(reason) = field.strip_prefix(b"prunable ") {
            entry.prunable = Some(utf8(reason, "Worktree prune reason")?);
        }
    }
    if let Some(mut entry) = row {
        entry.is_main = rows.is_empty();
        rows.push(entry);
    }
    Ok(rows)
}

fn live_primary(list: &GitWorktreeList) -> Result<&GitWorktree, ApiError> {
    let main = list.worktrees.iter().find(|row| row.is_main && !row.bare && row.head.is_some())
        .ok_or_else(|| ApiError::InvalidArgument("Worktree creation/migration requires a non-bare repository with a primary checkout and an existing commit".into()))?;
    if !Path::new(&main.path).is_dir() || main.prunable.is_some() {
        return Err(ApiError::InvalidArgument(
            "The primary checkout is missing; restore it before creating or migrating worktrees"
                .into(),
        ));
    }
    Ok(main)
}

fn linked_member<'a>(
    list: &'a GitWorktreeList,
    requested: &str,
) -> Result<&'a GitWorktree, ApiError> {
    let requested = absolute_path(Path::new(requested))?;
    let row = list
        .worktrees
        .iter()
        .find(|row| Path::new(&row.path) == requested)
        .ok_or_else(|| {
            ApiError::InvalidArgument(format!(
                "Not a registered worktree in this repository: {}",
                requested.display()
            ))
        })?;
    if row.is_main || row.bare {
        return Err(ApiError::InvalidArgument(
            "The primary/bare checkout cannot be removed or migrated".into(),
        ));
    }
    if row.prunable.is_some() || !Path::new(&row.path).is_dir() {
        return Err(ApiError::InvalidArgument(format!(
            "Worktree is missing; use Prune Worktrees instead: {}",
            row.path
        )));
    }
    Ok(row)
}

fn removable_member<'a>(
    list: &'a GitWorktreeList,
    requested: &str,
) -> Result<&'a GitWorktree, ApiError> {
    let row = linked_member(list, requested)?;
    if row.locked.is_some() {
        return Err(ApiError::InvalidArgument(format!(
            "Worktree is locked: {}",
            row.path
        )));
    }
    if list
        .worktrees
        .iter()
        .any(|other| other.path != row.path && Path::new(&other.path).starts_with(&row.path))
    {
        return Err(ApiError::InvalidArgument(format!(
            "Worktree contains another registered worktree: {}",
            row.path
        )));
    }
    if Path::new(&list.common_dir).starts_with(&row.path) {
        return Err(ApiError::InvalidArgument(
            "Worktree contains the shared Git metadata directory and cannot be removed".into(),
        ));
    }
    Ok(row)
}

fn destination_absent(
    path: &Path,
    list: &GitWorktreeList,
    source: &Path,
) -> Result<PathBuf, ApiError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => {
            return Err(ApiError::InvalidArgument(format!(
                "Destination already exists: {}",
                path.display()
            )))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(ApiError::Io(format!(
                "Cannot inspect destination {}: {error}",
                path.display()
            )))
        }
    }
    let path = absolute_path(path)?;
    if path == source || path.starts_with(&list.common_dir) {
        return Err(ApiError::InvalidArgument(
            "Destination must be separate from the source checkout and Git metadata".into(),
        ));
    }
    if list
        .worktrees
        .iter()
        .any(|row| Path::new(&row.path).starts_with(&path))
    {
        return Err(ApiError::InvalidArgument(
            "Destination contains an existing registered worktree".into(),
        ));
    }
    Ok(path)
}

fn branch_name(path: &Path, name: &str) -> Result<(), ApiError> {
    super::validated_ref_name(name)?;
    let output = git_command(path)
        .args(["check-ref-format", "--branch", name])
        .output()
        .map_err(|error| ApiError::Io(error.to_string()))?;
    if !output.status.success() {
        return Err(ApiError::InvalidArgument(format!(
            "Invalid branch name: {name}"
        )));
    }
    Ok(())
}

fn commit_oid(path: &Path, rev: &str) -> Result<String, ApiError> {
    if rev.is_empty() || rev.contains('\0') {
        return Err(ApiError::InvalidArgument(
            "Commit revision is empty or contains NUL".into(),
        ));
    }
    read_line(
        path,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{rev}^{{commit}}"),
        ],
        "resolve worktree commit",
    )
}

impl GitService {
    pub fn worktree_common_dir(&self, path: &Path) -> Result<String, ApiError> {
        let raw = read_line(
            path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            "resolve common Git directory",
        )?;
        path_string(&std::fs::canonicalize(&raw).map_err(|error| {
            ApiError::Io(format!("Cannot resolve Git directory {raw}: {error}"))
        })?)
    }

    pub fn worktree_checkout_root(&self, path: &Path) -> Result<PathBuf, ApiError> {
        let raw = read_line(
            path,
            &["rev-parse", "--show-toplevel"],
            "resolve invoking checkout",
        )?;
        let root = std::fs::canonicalize(&raw)
            .map_err(|error| ApiError::Io(format!("Cannot resolve checkout {raw}: {error}")))?;
        path_string(&root)?;
        Ok(root)
    }

    pub fn list_worktrees(&self, path: &Path) -> Result<GitWorktreeList, ApiError> {
        let common_dir = self.worktree_common_dir(path)?;
        let bare = read_line(
            path,
            &["rev-parse", "--is-bare-repository"],
            "inspect repository",
        )? == "true";
        let current = if bare {
            None
        } else {
            Some(self.worktree_checkout_root(path)?)
        };
        let bytes = git_output(
            path,
            &["worktree", "list", "--porcelain", "-z"],
            "list worktrees",
        )
        .map_err(ApiError::Internal)?;
        let worktrees = parse_inventory(&bytes, current.as_deref())?;
        let main_path = worktrees
            .iter()
            .find(|row| row.is_main && !row.bare)
            .map(|row| row.path.clone());
        Ok(GitWorktreeList {
            common_dir,
            main_path,
            worktrees,
        })
    }

    pub fn default_worktree_path(
        &self,
        path: &Path,
        checkout: &GitWorktreeCheckout,
        root: Option<&str>,
    ) -> Result<String, ApiError> {
        let list = self.list_worktrees(path)?;
        let main = live_primary(&list)?;
        let source = self.worktree_checkout_root(path)?;
        commit_oid(&source, "HEAD")?;
        let leaf = match checkout {
            GitWorktreeCheckout::ExistingBranch { name }
            | GitWorktreeCheckout::NewBranch { name, .. } => {
                branch_name(&source, name)?;
                name.replace('/', "-")
            }
            GitWorktreeCheckout::Detached { rev } => {
                let oid = commit_oid(&source, rev)?;
                format!(
                    "detached-{}",
                    read_line(
                        &source,
                        &["rev-parse", "--short=12", &oid],
                        "abbreviate commit"
                    )?
                )
            }
        };
        let root = match root {
            Some(root) => absolute_path(Path::new(root))?,
            None => {
                let main = Path::new(&main.path);
                let name = main.file_name().ok_or_else(|| {
                    ApiError::InvalidArgument(
                        "Primary checkout has no directory basename; choose a worktree root".into(),
                    )
                })?;
                let mut name = name.to_os_string();
                name.push(".worktrees");
                main.parent()
                    .ok_or_else(|| {
                        ApiError::InvalidArgument(
                            "Primary checkout has no parent; choose a worktree root".into(),
                        )
                    })?
                    .join(name)
            }
        };
        for suffix in 0_u64.. {
            let candidate = root.join(if suffix == 0 {
                leaf.clone()
            } else {
                format!("{leaf}-{suffix}")
            });
            if list
                .worktrees
                .iter()
                .any(|row| Path::new(&row.path) == candidate)
            {
                continue;
            }
            match std::fs::symlink_metadata(&candidate) {
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return path_string(&destination_absent(&candidate, &list, &source)?)
                }
                Err(error) => {
                    return Err(ApiError::Io(format!(
                        "Cannot inspect {}: {error}",
                        candidate.display()
                    )))
                }
            }
        }
        unreachable!("worktree suffix search exhausted")
    }

    pub fn worktree_affected_roots(
        &self,
        path: &Path,
        op: &GitWorktreeOp,
    ) -> Result<Vec<String>, ApiError> {
        let list = self.list_worktrees(path)?;
        match op {
            GitWorktreeOp::Remove { worktree_path, .. } => {
                Ok(vec![removable_member(&list, worktree_path)?.path.clone()])
            }
            GitWorktreeOp::Migrate { worktree_path } => Ok(vec![
                linked_member(&list, worktree_path)?.path.clone(),
                live_primary(&list)?.path.clone(),
            ]),
            _ => Ok(Vec::new()),
        }
    }

    pub fn worktree(
        &self,
        path: &Path,
        op: GitWorktreeOp,
        patterns: &BTreeMap<String, bool>,
    ) -> Result<GitWorktreeResult, ApiError> {
        let common = self.worktree_common_dir(path)?;
        let lock = {
            let mut locks = self.worktree_locks.lock();
            locks.retain(|_, lock| lock.strong_count() > 0);
            match locks.get(Path::new(&common)).and_then(Weak::upgrade) {
                Some(lock) => lock,
                None => {
                    let lock = Arc::new(parking_lot::Mutex::new(()));
                    locks.insert(PathBuf::from(&common), Arc::downgrade(&lock));
                    lock
                }
            }
        };
        let _guard = lock.lock();
        let list = self.list_worktrees(path)?;
        if list.common_dir != common {
            return Err(ApiError::InvalidArgument("Repository identity changed while admitting worktree mutation; refresh and try again".into()));
        }
        let result = match op {
            GitWorktreeOp::Add {
                destination,
                checkout,
            } => self.add_worktree(path, &list, &destination, checkout, patterns),
            GitWorktreeOp::Remove {
                worktree_path,
                force,
            } => {
                let member = removable_member(&list, &worktree_path)?;
                let root = Path::new(&member.path);
                if !force {
                    let status = self.working_status(root).map_err(ApiError::Internal)?;
                    let ignored = git_output(
                        root,
                        &[
                            "ls-files",
                            "--others",
                            "--ignored",
                            "--exclude-standard",
                            "-z",
                            "--",
                        ],
                        "inspect ignored worktree files",
                    )
                    .map_err(ApiError::Internal)?;
                    if !status.changes.is_empty() || !ignored.is_empty() {
                        return Err(ApiError::WorktreeHasChanges {
                            path: member.path.clone(),
                            message: format!(
                                "Worktree contains modified, untracked or ignored files: {}",
                                member.path
                            ),
                        });
                    }
                }
                let args = if force {
                    vec!["worktree", "remove", "--force", "--", member.path.as_str()]
                } else {
                    vec!["worktree", "remove", "--", member.path.as_str()]
                };
                // Never run the remover with cwd inside the directory it deletes.
                run_git_mutate(Path::new(&list.common_dir), &args)
                    .map(|()| GitWorktreeResult::Removed)
            }
            GitWorktreeOp::Prune => run_git_mutate(
                Path::new(&list.common_dir),
                &["worktree", "prune", "--expire=now"],
            )
            .map(|()| GitWorktreeResult::Pruned),
            GitWorktreeOp::Migrate { worktree_path } => {
                self.migrate_worktree(&list, &worktree_path)
            }
        };
        // Partial migration/add failures can change state too. Never leave a pre-
        // transaction summary cached, including folders opened below a root.
        let paths: Vec<PathBuf> = self.summary_cache.lock().keys().cloned().collect();
        for cached in paths {
            if list
                .worktrees
                .iter()
                .any(|row| cached.starts_with(&row.path))
            {
                self.invalidate(&cached);
                self.ahead_behind_cache.lock().remove(&cached);
            }
        }
        for row in &list.worktrees {
            self.invalidate(Path::new(&row.path));
        }
        result
    }

    fn add_worktree(
        &self,
        path: &Path,
        list: &GitWorktreeList,
        destination: &str,
        checkout: GitWorktreeCheckout,
        patterns: &BTreeMap<String, bool>,
    ) -> Result<GitWorktreeResult, ApiError> {
        live_primary(list)?;
        let source = self.worktree_checkout_root(path)?;
        commit_oid(&source, "HEAD")?;
        let destination = destination_absent(Path::new(destination), list, &source)?;
        let destination_text = path_string(&destination)?;
        let oid = match &checkout {
            GitWorktreeCheckout::ExistingBranch { name } => {
                branch_name(&source, name)?;
                let full_ref = format!("refs/heads/{name}");
                let oid = commit_oid(&source, &full_ref)?;
                if let Some(occupied) = list
                    .worktrees
                    .iter()
                    .find(|row| row.branch.as_deref() == Some(name.as_str()))
                {
                    return Err(ApiError::BranchInWorktree {
                        branch: name.clone(),
                        path: occupied.path.clone(),
                    });
                }
                oid
            }
            GitWorktreeCheckout::NewBranch { name, base } => {
                branch_name(&source, name)?;
                commit_oid(&source, base)?
            }
            GitWorktreeCheckout::Detached { rev } => commit_oid(&source, rev)?,
        };
        let snapshot = copy::snapshot(&source, list, patterns)?;
        std::fs::create_dir_all(
            destination
                .parent()
                .ok_or_else(|| ApiError::InvalidArgument("Destination has no parent".into()))?,
        )
        .map_err(|error| ApiError::Io(format!("Cannot create destination parent: {error}")))?;
        // Explicit input is never renamed. Recheck after preparation and just
        // before Git, while the common-repository transaction lock is held.
        if destination_absent(&destination, list, &source)? != destination {
            return Err(ApiError::InvalidArgument(
                "Destination parent changed while preparing creation; choose the destination again"
                    .into(),
            ));
        }
        let mut args = vec!["worktree", "add"];
        match &checkout {
            GitWorktreeCheckout::NewBranch { name, .. } => args.extend([
                "-b",
                name.as_str(),
                "--",
                destination_text.as_str(),
                oid.as_str(),
            ]),
            GitWorktreeCheckout::ExistingBranch { name } => {
                args.extend(["--", destination_text.as_str(), name.as_str()])
            }
            GitWorktreeCheckout::Detached { .. } => {
                args.extend(["--detach", "--", destination_text.as_str(), oid.as_str()])
            }
        }
        if let Err(error) = run_git_mutate(&source, &args) {
            // Translate concurrent occupancy from fresh porcelain, not quoted
            // stderr paths (which can contain quotes/newlines).
            if let GitWorktreeCheckout::ExistingBranch { name } = &checkout {
                if let Ok(fresh) = self.list_worktrees(&source) {
                    if let Some(row) = fresh
                        .worktrees
                        .iter()
                        .find(|row| row.branch.as_deref() == Some(name.as_str()))
                    {
                        return Err(ApiError::BranchInWorktree {
                            branch: name.clone(),
                            path: row.path.clone(),
                        });
                    }
                }
            }
            return Err(error);
        }
        let (copy_complete, warnings) = copy::apply(snapshot, &destination);
        Ok(GitWorktreeResult::Added {
            path: destination_text,
            copy_complete,
            warnings,
        })
    }

    fn migration_status(&self, path: &Path) -> Result<super::WorkingStatus, ApiError> {
        let status = self.working_status(path).map_err(ApiError::Internal)?;
        if !status.conflicts.is_empty() {
            return Err(ApiError::MergeConflicts {
                message: format!(
                    "Resolve unmerged paths before migration: {}",
                    path.display()
                ),
            });
        }
        let git_dir = read_line(
            path,
            &["rev-parse", "--absolute-git-dir"],
            "resolve checkout Git directory",
        )?;
        if read_operation(path, Path::new(&git_dir)).is_some() {
            return Err(ApiError::InvalidArgument(format!(
                "Finish the paused Git operation before migration: {}",
                path.display()
            )));
        }
        Ok(status)
    }

    fn migrate_worktree(
        &self,
        list: &GitWorktreeList,
        requested: &str,
    ) -> Result<GitWorktreeResult, ApiError> {
        let source = Path::new(&linked_member(list, requested)?.path);
        let main = Path::new(&live_primary(list)?.path);
        let source_status = self.migration_status(source)?;
        let main_status = self.migration_status(main)?;
        if !main_status.changes.is_empty() {
            return Err(ApiError::DirtyWorktree {
                message: format!(
                    "Main checkout must be clean before migration: {}",
                    main.display()
                ),
            });
        }
        ensure_nested_clean(source)?;
        ensure_nested_clean(main)?;
        if source_status.changes.is_empty() {
            return Ok(GitWorktreeResult::Migrated {
                main_path: path_string(main)?,
                recovery_stash: None,
            });
        }
        let source_head = commit_oid(source, "HEAD")?;
        let label = format!("Sworm Worktree Migration {}", uuid::Uuid::new_v4());
        let pushed = run_git_mutate(
            source,
            &["stash", "push", "--include-untracked", "-m", &label],
        );
        let recovery = recovery_oid(source, &label);
        if let Err(error) = pushed {
            let recovery_detail = match recovery {
                Ok(Some(oid)) => format!("Recovery stash retained: {oid} ({label})."),
                Ok(None) => format!("No recovery stash with label {label} was found."),
                Err(error) => {
                    format!("Recovery stash lookup failed: {error}; inspect label {label}.")
                }
            };
            return Err(recovery_error(error, source, main, &recovery_detail));
        }
        let oid = recovery.map_err(|error| recovery_error(error, source, main, &format!("Stash push completed, but its OID could not be read; inspect recovery label {label}.")))?
            .ok_or_else(|| ApiError::Internal(format!("Stash push completed but recovery label {label} was not found. Source: {}; main: {}. No further mutation attempted.", source.display(), main.display())))?;
        let captured_source = self
            .migration_status(source)
            .and_then(|status| {
                if status.changes.is_empty() {
                    Ok(())
                } else {
                    Err(ApiError::InvalidArgument(
                        "Source changed while capturing migration".into(),
                    ))
                }
            })
            .and_then(|()| {
                if commit_oid(source, "HEAD")? == source_head {
                    Ok(())
                } else {
                    Err(ApiError::InvalidArgument(
                        "Source HEAD changed while capturing migration".into(),
                    ))
                }
            });
        if let Err(error) = captured_source {
            return Err(recovery_error(
                error,
                source,
                main,
                &format!("No application attempted. Recovery stash retained: {oid} ({label})."),
            ));
        }
        // External writers are not controlled by admission. Recheck immediately
        // before applying rather than overwriting a newly dirty destination.
        let main_recheck = self.migration_status(main).and_then(|status| {
            if status.changes.is_empty() {
                Ok(())
            } else {
                Err(ApiError::DirtyWorktree {
                    message: "Main changed while capturing migration".into(),
                })
            }
        });
        let apply = match main_recheck {
            Ok(()) => ensure_no_ignored_collisions(main, &oid)
                .and_then(|()| run_git_mutate(main, &["stash", "apply", "--index", &oid])),
            Err(error) => Err(error),
        };
        match apply {
            Ok(()) => Ok(GitWorktreeResult::Migrated {
                main_path: path_string(main)?,
                recovery_stash: Some(oid),
            }),
            Err(error) => {
                let restore = self.restore_migration_source(source, &source_head, &oid);
                let restored = match restore {
                    Ok(()) => "Source changes restored with staging preserved.".to_string(),
                    Err(error) => format!("Source restore refused/failed: {error}. Source state retained; recover manually from the stash."),
                };
                let (error, main_status) = match self.working_status(main) {
                    Ok(status) if !status.conflicts.is_empty() => (
                        ApiError::MergeConflicts {
                            message: error.to_string(),
                        },
                        "Main has unmerged paths.".to_owned(),
                    ),
                    Ok(_) if matches!(error, ApiError::MergeConflicts { .. }) => (
                        ApiError::Internal(error.to_string()),
                        "Main has no unmerged paths.".to_owned(),
                    ),
                    Ok(_) => (error, "Main has no unmerged paths.".to_owned()),
                    Err(status_error) => (
                        if matches!(error, ApiError::MergeConflicts { .. }) {
                            ApiError::Internal(error.to_string())
                        } else {
                            error
                        },
                        format!("Main status could not be read: {status_error}."),
                    ),
                };
                Err(recovery_error(error, source, main, &format!("{restored} {main_status} Main state has not been reset. Recovery stash retained: {oid} ({label}).")))
            }
        }
    }

    fn restore_migration_source(
        &self,
        source: &Path,
        head: &str,
        oid: &str,
    ) -> Result<(), ApiError> {
        if commit_oid(source, "HEAD")? != head {
            return Err(ApiError::InvalidArgument(
                "Source HEAD changed since capture; it will not be overwritten".into(),
            ));
        }
        if !self.migration_status(source)?.changes.is_empty() {
            return Err(ApiError::InvalidArgument(
                "Source changed since capture; it will not be overwritten".into(),
            ));
        }
        ensure_no_ignored_collisions(source, oid)?;
        run_git_mutate(source, &["stash", "apply", "--index", oid])
    }
}

fn recovery_oid(path: &Path, label: &str) -> Result<Option<String>, ApiError> {
    let exists = git_command(path)
        .args(["show-ref", "--verify", "--quiet", "refs/stash"])
        .output()
        .map_err(|error| ApiError::Io(error.to_string()))?;
    if exists.status.code() == Some(1) {
        return Ok(None);
    }
    if !exists.status.success() {
        return Err(super::mutation_error(&exists));
    }
    let bytes = git_output(
        path,
        &["reflog", "show", "--format=%H%x00%gs", "-z", "refs/stash"],
        "locate migration recovery stash",
    )
    .map_err(ApiError::Internal)?;
    let fields: Vec<&[u8]> = bytes.split(|byte| *byte == 0).collect();
    for pair in fields.chunks_exact(2) {
        let subject = utf8(pair[1], "Stash reflog subject")?;
        if subject == label || subject.ends_with(&format!(": {label}")) {
            return Ok(Some(utf8(pair[0], "Recovery stash OID")?));
        }
    }
    Ok(None)
}

fn recovery_error(mut error: ApiError, source: &Path, main: &Path, detail: &str) -> ApiError {
    let suffix = format!(
        "\nSource: {}\nMain: {}\n{detail}",
        source.display(),
        main.display()
    );
    match &mut error {
        ApiError::Internal(message)
        | ApiError::Io(message)
        | ApiError::InvalidArgument(message)
        | ApiError::Database(message)
        | ApiError::Pty(message)
        | ApiError::NotFound(message)
        | ApiError::Remote(message)
        | ApiError::DirtyWorktree { message }
        | ApiError::MergeConflicts { message }
        | ApiError::WorktreeHasChanges { message, .. }
        | ApiError::PushRejected { message }
        | ApiError::BranchUnmerged { message, .. }
        | ApiError::TrashUnavailable { message } => message.push_str(&suffix),
        _ => {}
    }
    error
}

/// Git stash cannot capture nested working trees. Check gitlinks and untracked
/// directory records, without following arbitrary filesystem symlinks.
fn ensure_nested_clean(root: &Path) -> Result<(), ApiError> {
    let staged = git_output(
        root,
        &["ls-files", "--stage", "-z", "--"],
        "inspect nested repositories",
    )
    .map_err(ApiError::Internal)?;
    let mut nested = Vec::new();
    for record in staged
        .split(|byte| *byte == 0)
        .filter(|record| record.starts_with(b"160000 "))
    {
        if let Some(index) = record.iter().position(|byte| *byte == b'\t') {
            let relative = utf8(&record[index + 1..], "Submodule path")?;
            let path = root.join(&relative);
            if path.join(".git").exists() {
                let changes = git_output(
                    root,
                    &[
                        "diff",
                        "HEAD",
                        "--name-only",
                        "-z",
                        "--ignore-submodules=none",
                        "--",
                        &relative,
                    ],
                    "inspect submodule changes",
                )
                .map_err(ApiError::Internal)?;
                if !changes.is_empty() {
                    return Err(ApiError::InvalidArgument(format!(
                        "Commit or move changes in submodule before migration: {}",
                        path.display()
                    )));
                }
                nested.push(path);
            }
        }
    }
    let others = git_output(
        root,
        &[
            "ls-files",
            "--others",
            "--directory",
            "--exclude-standard",
            "-z",
            "--",
        ],
        "inspect untracked repositories",
    )
    .map_err(ApiError::Internal)?;
    for raw in others
        .split(|byte| *byte == 0)
        .filter(|raw| raw.ends_with(b"/"))
    {
        let path = root.join(utf8(raw, "Nested repository path")?);
        collect_nested_repositories(&path, &mut nested)?;
    }
    nested.sort();
    nested.dedup();
    for path in nested {
        if !path.is_dir() {
            continue;
        }
        if read_line(
            &path,
            &["rev-parse", "--is-bare-repository"],
            "inspect nested repository",
        )? == "true"
        {
            return Err(ApiError::InvalidArgument(format!(
                "Untracked nested bare repository cannot be captured by migration: {}",
                path.display()
            )));
        }
        let status = git_output(
            &path,
            &[
                "status",
                "--porcelain=v2",
                "-z",
                "--untracked-files=all",
                "--ignore-submodules=none",
            ],
            "inspect nested repository changes",
        )
        .map_err(ApiError::Internal)?;
        if !status.is_empty() {
            return Err(ApiError::InvalidArgument(format!(
                "Commit or move changes in nested repository before migration: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn collect_nested_repositories(path: &Path, nested: &mut Vec<PathBuf>) -> Result<(), ApiError> {
    let meta = std::fs::symlink_metadata(path).map_err(|error| ApiError::Io(error.to_string()))?;
    if !meta.is_dir() {
        return Ok(());
    }
    if path.join(".git").exists() || (path.join("HEAD").is_file() && path.join("objects").is_dir())
    {
        nested.push(path.to_path_buf());
        return Ok(());
    }
    for entry in std::fs::read_dir(path).map_err(|error| ApiError::Io(error.to_string()))? {
        let entry = entry.map_err(|error| ApiError::Io(error.to_string()))?;
        if entry
            .file_type()
            .map_err(|error| ApiError::Io(error.to_string()))?
            .is_dir()
        {
            collect_nested_repositories(&entry.path(), nested)?;
        }
    }
    Ok(())
}

fn ensure_no_ignored_collisions(main: &Path, oid: &str) -> Result<(), ApiError> {
    let ignored = git_output(
        main,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "-z",
            "--",
        ],
        "inspect ignored destination files",
    )
    .map_err(ApiError::Internal)?;
    if ignored.is_empty() {
        return Ok(());
    }
    let tracked = git_output(
        main,
        &["diff", "--name-only", "-z", &format!("{oid}^1"), oid, "--"],
        "inspect migration paths",
    )
    .map_err(ApiError::Internal)?;
    let parents = read_line(
        main,
        &["rev-list", "--parents", "-n", "1", oid],
        "inspect migration stash parents",
    )?;
    let untracked = match parents.split_whitespace().nth(3) {
        Some(parent) => git_output(
            main,
            &["ls-tree", "-r", "--name-only", "-z", parent],
            "inspect migration untracked paths",
        )
        .map_err(ApiError::Internal)?,
        None => Vec::new(),
    };
    let mut paths = Vec::new();
    for raw in tracked
        .split(|byte| *byte == 0)
        .chain(untracked.split(|byte| *byte == 0))
        .filter(|raw| !raw.is_empty())
    {
        paths.push(PathBuf::from(utf8(raw, "Migration path")?));
    }
    for raw in ignored
        .split(|byte| *byte == 0)
        .filter(|raw| !raw.is_empty())
    {
        let ignored = PathBuf::from(utf8(raw, "Ignored destination path")?);
        if paths
            .iter()
            .any(|path| path.starts_with(&ignored) || ignored.starts_with(path))
        {
            return Err(ApiError::InvalidArgument(format!("Ignored destination file would collide with migration and will not be overwritten: {}", main.join(ignored).display())));
        }
    }
    Ok(())
}
