use super::*;
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::sync::Barrier;

struct Repo {
    parent: PathBuf,
    main: PathBuf,
}

impl Repo {
    fn new(tag: &str) -> Self {
        Self::with_main_name(tag, "main")
    }

    fn with_main_name(tag: &str, name: &str) -> Self {
        let parent = Path::new("/tmp").join(format!(
            "sworm-worktree-{tag}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let main = parent.join(name);
        fs::create_dir_all(&main).unwrap();
        let repo = Self { parent, main };
        git(&repo.main, &["init", "-q", "-b", "main"]);
        git(&repo.main, &["config", "user.name", "Worktree Test"]);
        git(
            &repo.main,
            &["config", "user.email", "worktree@test.invalid"],
        );
        git(&repo.main, &["config", "commit.gpgsign", "false"]);
        git(&repo.main, &["config", "core.autocrlf", "false"]);
        let hooks = repo.parent.join("empty-hooks");
        fs::create_dir(&hooks).unwrap();
        git(&repo.main, &["config", "core.hooksPath", text(&hooks)]);
        write_file(&repo.main.join("a.txt"), b"one\n");
        write_file(&repo.main.join("b.txt"), b"base b\n");
        write_file(&repo.main.join(".gitignore"), b"ignored/\n");
        git(&repo.main, &["add", "a.txt", "b.txt", ".gitignore"]);
        git(&repo.main, &["commit", "-q", "-m", "initial"]);
        repo
    }

    fn linked(&self, branch: &str, leaf: &str) -> PathBuf {
        let path = self.parent.join(leaf);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        git(
            &self.main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                branch,
                "--",
                text(&path),
                "main",
            ],
        );
        path
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.parent);
    }
}

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn write_file(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn git(path: &Path, args: &[&str]) -> Vec<u8> {
    let output = git_command(path).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?} in {path:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn line(path: &Path, args: &[&str]) -> String {
    let mut bytes = git(path, args);
    assert_eq!(bytes.pop(), Some(b'\n'));
    String::from_utf8(bytes).unwrap()
}

fn diffs(path: &Path) -> (Vec<u8>, Vec<u8>) {
    (
        git(
            path,
            &["diff", "--cached", "--binary", "--no-ext-diff", "--"],
        ),
        git(path, &["diff", "--binary", "--no-ext-diff", "--"]),
    )
}

fn row<'a>(list: &'a GitWorktreeList, path: &Path) -> &'a GitWorktree {
    list.worktrees
        .iter()
        .find(|row| row.path == text(path))
        .unwrap()
}

fn add(
    service: &GitService,
    source: &Path,
    destination: &Path,
    checkout: GitWorktreeCheckout,
) -> Result<GitWorktreeResult, ApiError> {
    service.worktree(
        source,
        GitWorktreeOp::Add {
            destination: text(destination).into(),
            checkout,
        },
        &BTreeMap::new(),
    )
}

fn assert_added(result: GitWorktreeResult, destination: &Path) {
    match result {
        GitWorktreeResult::Added {
            path,
            copy_complete,
            warnings,
        } => {
            assert_eq!(path, text(destination));
            assert!(copy_complete);
            assert!(warnings.is_empty(), "{warnings:?}");
        }
        other => panic!("Expected Added, got {other:?}"),
    }
}

fn assert_invalid<T: std::fmt::Debug>(result: Result<T, ApiError>) {
    assert!(
        matches!(result, Err(ApiError::InvalidArgument(_))),
        "{result:?}"
    );
}

fn prior_stash(path: &Path) -> String {
    write_file(&path.join("a.txt"), b"preexisting stash bytes\n");
    git(path, &["stash", "push", "-q", "-m", "preexisting stash"]);
    commit_oid(path, "refs/stash").unwrap()
}

fn assert_recovery_stash(path: &Path, oid: &str) -> String {
    assert_eq!(oid, commit_oid(path, oid).unwrap());
    assert_eq!(oid.len(), commit_oid(path, "HEAD").unwrap().len());
    assert!(oid.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let subject = line(path, &["show", "-s", "--format=%s", oid]);
    let (_, uuid) = subject.rsplit_once("Sworm Worktree Migration ").unwrap();
    assert_eq!(uuid::Uuid::parse_str(uuid).unwrap().get_version_num(), 4);
    let label = format!("Sworm Worktree Migration {uuid}");
    assert_eq!(recovery_oid(path, &label).unwrap().as_deref(), Some(oid));
    assert_eq!(commit_oid(path, "refs/stash").unwrap(), oid);
    label
}

#[test]
fn inventory_preserves_whitespace_and_main_linked_detached_locked_prunable_records() {
    let repo = Repo::with_main_name("inventory", " main space\tline\n ");
    let service = GitService::new();
    let linked = repo.parent.join("linked space\tline\n ");
    let detached = repo.parent.join("detached space\tline\n ");
    assert_added(
        add(
            &service,
            &repo.main,
            &linked,
            GitWorktreeCheckout::NewBranch {
                name: "topic/linked".into(),
                base: "main".into(),
            },
        )
        .unwrap(),
        &linked,
    );
    assert_added(
        add(
            &service,
            &repo.main,
            &detached,
            GitWorktreeCheckout::Detached { rev: "main".into() },
        )
        .unwrap(),
        &detached,
    );
    let locked = repo.linked("topic/locked", "locked space\tline\n ");
    let stale = repo.linked("topic/stale", "stale space\tline\n ");
    let reason = "lock reason\twith\nnewlines";
    git(
        &repo.main,
        &["worktree", "lock", "--reason", reason, "--", text(&locked)],
    );
    fs::remove_dir_all(&stale).unwrap();

    let list = service.list_worktrees(&repo.main).unwrap();
    assert_eq!(list.common_dir, text(&repo.main.join(".git")));
    assert_eq!(list.main_path.as_deref(), Some(text(&repo.main)));
    assert_eq!(list.worktrees.len(), 5);
    assert_eq!(list.worktrees.iter().filter(|row| row.is_main).count(), 1);
    assert_eq!(
        list.worktrees.iter().filter(|row| row.is_current).count(),
        1
    );
    let head = commit_oid(&repo.main, "HEAD").unwrap();
    for path in [&repo.main, &linked, &detached, &locked, &stale] {
        let entry = row(&list, path);
        assert_eq!(entry.head.as_deref(), Some(head.as_str()));
        assert!(!entry.bare);
        assert_eq!(entry.is_main, path == &repo.main);
        assert_eq!(entry.is_current, path == &repo.main);
    }
    assert_eq!(row(&list, &repo.main).branch.as_deref(), Some("main"));
    assert_eq!(row(&list, &linked).branch.as_deref(), Some("topic/linked"));
    assert!(row(&list, &linked).locked.is_none());
    assert!(row(&list, &linked).prunable.is_none());
    assert!(row(&list, &detached).branch.is_none());
    assert_eq!(row(&list, &locked).locked.as_deref(), Some(reason));
    assert!(row(&list, &stale)
        .prunable
        .as_deref()
        .is_some_and(|reason| !reason.is_empty()));

    let raw = git(&repo.main, &["worktree", "list", "--porcelain", "-z"]);
    let parsed = parse_inventory(&raw, Some(&detached)).unwrap();
    assert_eq!(parsed.len(), 5);
    assert_eq!(parsed.iter().filter(|row| row.is_current).count(), 1);
    assert!(
        parsed
            .iter()
            .find(|row| row.path == text(&detached))
            .unwrap()
            .is_current
    );
    assert_eq!(
        parsed
            .iter()
            .find(|row| row.path == text(&locked))
            .unwrap()
            .locked
            .as_deref(),
        Some(reason)
    );
}

#[test]
fn bare_and_unborn_inventory_display_but_refuse_creation() {
    let repo = Repo::new("unsupported");
    let bare = repo.parent.join("bare space\tline\n ");
    let unborn = repo.parent.join("unborn space\tline\n ");
    for path in [&bare, &unborn] {
        fs::create_dir(path).unwrap();
    }
    git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    git(&unborn, &["init", "-q", "-b", "main"]);
    let service = GitService::new();
    let bare_list = service.list_worktrees(&bare).unwrap();
    assert_eq!(bare_list.common_dir, text(&bare));
    assert!(bare_list.main_path.is_none());
    assert_eq!(bare_list.worktrees.len(), 1);
    let bare_row = row(&bare_list, &bare);
    assert!(bare_row.is_main && bare_row.bare && !bare_row.is_current);
    assert!(bare_row.head.is_none() && bare_row.branch.is_none());
    let unborn_list = service.list_worktrees(&unborn).unwrap();
    assert_eq!(unborn_list.main_path.as_deref(), Some(text(&unborn)));
    assert_eq!(unborn_list.worktrees.len(), 1);
    let unborn_row = row(&unborn_list, &unborn);
    assert!(unborn_row.is_main && unborn_row.is_current && !unborn_row.bare);
    assert!(unborn_row.head.is_none());
    assert_eq!(unborn_row.branch.as_deref(), Some("main"));

    for (index, source) in [&bare, &unborn].into_iter().enumerate() {
        for checkout in [
            GitWorktreeCheckout::ExistingBranch {
                name: "main".into(),
            },
            GitWorktreeCheckout::NewBranch {
                name: "topic".into(),
                base: "main".into(),
            },
            GitWorktreeCheckout::Detached { rev: "HEAD".into() },
        ] {
            let destination = repo.parent.join(format!("unsupported-{index}"));
            assert_invalid(service.default_worktree_path(source, &checkout, None));
            assert_invalid(add(&service, source, &destination, checkout));
            assert!(!destination.exists());
            assert_eq!(service.list_worktrees(source).unwrap().worktrees.len(), 1);
        }
    }
}

#[test]
fn non_utf8_filesystem_worktree_paths_are_rejected() {
    let repo = Repo::new("non-utf8");
    let path = repo
        .parent
        .join(OsString::from_vec(b"linked-\xff".to_vec()));
    let output = git_command(&repo.main)
        .args(["worktree", "add", "-q", "--detach", "--"])
        .arg(&path)
        .arg("HEAD")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(path.is_dir());
    assert_invalid(absolute_path(&path));
    assert_invalid(absolute_path(&path.join("missing")));
    assert_invalid(GitService::new().list_worktrees(&repo.main));
    let error = GitService::new().list_worktrees(&path).unwrap_err();
    assert!(error.to_string().contains("UTF-8"), "{error:?}");

    let branches = GitService::new().list_branches(&repo.main).unwrap();
    let main = branches
        .iter()
        .find(|branch| branch.name == "main")
        .unwrap();
    assert!(main.is_current);
    assert_eq!(main.worktree_path, None);
}

#[test]
fn branch_list_reports_current_and_linked_worktree_paths() {
    let repo = Repo::new("branch-paths");
    let linked = repo.linked("topic", "linked space\tline\n ");
    git(&repo.main, &["branch", "unused"]);
    git(
        &repo.main,
        &["update-ref", "refs/remotes/origin/topic", "HEAD"],
    );
    let service = GitService::new();
    for (source, current) in [(&repo.main, "main"), (&linked, "topic")] {
        let branches = service.list_branches(source).unwrap();
        for (name, path) in [
            ("main", Some(text(&repo.main))),
            ("topic", Some(text(&linked))),
            ("unused", None),
            ("origin/topic", None),
        ] {
            let branch = branches.iter().find(|branch| branch.name == name).unwrap();
            assert_eq!(branch.worktree_path.as_deref(), path);
            assert_eq!(branch.is_current, name == current);
        }
    }
}

#[test]
fn default_path_skips_registered_filesystem_and_dangling_symlink_collisions() {
    let repo = Repo::new("default-collisions");
    let root = repo.parent.join("main.worktrees");
    let registered = repo.linked("occupied", "main.worktrees/feature-task");
    fs::remove_dir_all(&registered).unwrap();
    fs::create_dir(root.join("feature-task-1")).unwrap();
    symlink(
        repo.parent.join("missing-target"),
        root.join("feature-task-2"),
    )
    .unwrap();
    let checkout = GitWorktreeCheckout::NewBranch {
        name: "feature/task".into(),
        base: "main".into(),
    };
    let service = GitService::new();
    let expected = root.join("feature-task-3");
    assert_eq!(
        service
            .default_worktree_path(&repo.main, &checkout, None)
            .unwrap(),
        text(&expected)
    );
    assert_eq!(
        service
            .default_worktree_path(&repo.main, &checkout, Some(text(&root)))
            .unwrap(),
        text(&expected)
    );
    assert!(!expected.exists());
    assert!(fs::symlink_metadata(root.join("feature-task-2"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(
        row(&service.list_worktrees(&repo.main).unwrap(), &registered)
            .prunable
            .is_some()
    );
}

#[test]
fn explicit_add_refuses_existing_source_metadata_and_registered_descendants() {
    let repo = Repo::new("explicit-destinations");
    let registered = repo.linked("stale-child", "absent-container/registered-child");
    let container = registered.parent().unwrap();
    fs::remove_dir_all(container).unwrap();
    let existing = repo.parent.join("existing");
    write_file(&existing.join("sentinel"), b"keep existing bytes\0");
    let service = GitService::new();
    let metadata = repo.main.join(".git/new-checkout");
    for (index, destination) in [
        existing.as_path(),
        repo.main.as_path(),
        metadata.as_path(),
        container,
    ]
    .into_iter()
    .enumerate()
    {
        let branch = format!("rejected-{index}");
        assert_invalid(add(
            &service,
            &repo.main,
            destination,
            GitWorktreeCheckout::NewBranch {
                name: branch.clone(),
                base: "main".into(),
            },
        ));
        assert!(!git_command(&repo.main)
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}")
            ])
            .status()
            .unwrap()
            .success());
    }
    assert_eq!(
        fs::read(existing.join("sentinel")).unwrap(),
        b"keep existing bytes\0"
    );
    assert_eq!(fs::read(repo.main.join("a.txt")).unwrap(), b"one\n");
    assert!(!metadata.exists());
    assert!(!container.exists());
    assert_eq!(
        service.list_worktrees(&repo.main).unwrap().worktrees.len(),
        2
    );
}

#[test]
fn invalid_branch_revision_and_patterns_leave_no_checkout_or_branch() {
    let repo = Repo::new("invalid-add");
    let service = GitService::new();
    for (index, checkout) in [
        GitWorktreeCheckout::NewBranch {
            name: "bad..branch".into(),
            base: "main".into(),
        },
        GitWorktreeCheckout::ExistingBranch {
            name: "--bad".into(),
        },
        GitWorktreeCheckout::NewBranch {
            name: "bad-base".into(),
            base: "missing-revision".into(),
        },
        GitWorktreeCheckout::Detached {
            rev: "missing-revision".into(),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let destination = repo.parent.join(format!("uncreated-{index}/checkout"));
        assert!(add(&service, &repo.main, &destination, checkout).is_err());
        assert!(!destination.parent().unwrap().exists());
    }
    for enabled in [false, true] {
        let branch = format!("invalid-pattern-{enabled}");
        let destination = repo.parent.join(format!("pattern-{enabled}/checkout"));
        let patterns = BTreeMap::from([("[".into(), enabled)]);
        assert_invalid(service.worktree(
            &repo.main,
            GitWorktreeOp::Add {
                destination: text(&destination).into(),
                checkout: GitWorktreeCheckout::NewBranch {
                    name: branch.clone(),
                    base: "main".into(),
                },
            },
            &patterns,
        ));
        assert!(!destination.parent().unwrap().exists());
        assert!(!git_command(&repo.main)
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}")
            ])
            .status()
            .unwrap()
            .success());
    }
    assert_eq!(
        line(
            &repo.main,
            &["for-each-ref", "--format=%(refname)", "refs/heads"]
        ),
        "refs/heads/main"
    );
    assert_eq!(
        service.list_worktrees(&repo.main).unwrap().worktrees.len(),
        1
    );
}

#[test]
fn subdirectory_invocation_uses_canonical_current_checkout_root() {
    let repo = Repo::new("subdirectory");
    let source = repo.linked("source", "linked source");
    write_file(&source.join("a.txt"), b"linked source commit\n");
    git(&source, &["commit", "-q", "-am", "source commit"]);
    let source_head = commit_oid(&source, "HEAD").unwrap();
    assert_ne!(source_head, commit_oid(&repo.main, "HEAD").unwrap());
    fs::create_dir_all(source.join("sub dir/deep")).unwrap();
    let alias = repo.parent.join("source-alias");
    symlink(&source, &alias).unwrap();
    let invocation = alias.join("sub dir/deep");
    let service = GitService::new();
    assert_eq!(service.worktree_checkout_root(&invocation).unwrap(), source);
    let list = service.list_worktrees(&invocation).unwrap();
    assert_eq!(list.main_path.as_deref(), Some(text(&repo.main)));
    assert!(row(&list, &source).is_current);
    assert!(!row(&list, &repo.main).is_current);
    let checkout = GitWorktreeCheckout::NewBranch {
        name: "from/subdir".into(),
        base: "HEAD".into(),
    };
    let destination = repo.parent.join("main.worktrees/from-subdir");
    assert_eq!(
        service
            .default_worktree_path(&invocation, &checkout, None)
            .unwrap(),
        text(&destination)
    );
    assert_added(
        add(&service, &invocation, &destination, checkout).unwrap(),
        &destination,
    );
    assert_eq!(commit_oid(&destination, "HEAD").unwrap(), source_head);
    assert_eq!(
        fs::read(destination.join("a.txt")).unwrap(),
        b"linked source commit\n"
    );
}

#[test]
fn concurrent_existing_branch_add_has_one_winner_and_one_occupied_error() {
    let repo = Repo::new("concurrent-add");
    git(&repo.main, &["branch", "topic"]);
    let subdir = repo.main.join("subdir");
    fs::create_dir(&subdir).unwrap();
    let destinations = [repo.parent.join("first"), repo.parent.join("second")];
    let service = Arc::new(GitService::new());
    let barrier = Arc::new(Barrier::new(3));
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = [&repo.main, &subdir]
            .into_iter()
            .zip(&destinations)
            .map(|(source, destination)| {
                let service = Arc::clone(&service);
                let barrier = Arc::clone(&barrier);
                scope.spawn(move || {
                    barrier.wait();
                    add(
                        &service,
                        source,
                        destination,
                        GitWorktreeCheckout::ExistingBranch {
                            name: "topic".into(),
                        },
                    )
                })
            })
            .collect();
        barrier.wait();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    let winners: Vec<_> = results
        .iter()
        .enumerate()
        .filter_map(|(index, result)| result.is_ok().then_some(index))
        .collect();
    assert_eq!(winners.len(), 1, "{results:?}");
    let winner = winners[0];
    let loser = 1 - winner;
    match &results[winner] {
        Ok(GitWorktreeResult::Added {
            path,
            copy_complete: true,
            warnings,
        }) => {
            assert_eq!(path, text(&destinations[winner]));
            assert!(warnings.is_empty());
        }
        other => panic!("Unexpected winner: {other:?}"),
    }
    match &results[loser] {
        Err(ApiError::BranchInWorktree { branch, path }) => {
            assert_eq!(branch, "topic");
            assert_eq!(path, text(&destinations[winner]));
        }
        other => panic!("Unexpected loser: {other:?}"),
    }
    assert!(!destinations[loser].exists());
    let list = service.list_worktrees(&repo.main).unwrap();
    assert_eq!(list.worktrees.len(), 2);
    assert_eq!(
        list.worktrees
            .iter()
            .filter(|row| row.branch.as_deref() == Some("topic"))
            .count(),
        1
    );
    assert_eq!(
        commit_oid(&destinations[winner], "HEAD").unwrap(),
        commit_oid(&repo.main, "HEAD").unwrap()
    );
}

#[test]
fn remove_refuses_primary_locked_registered_parent_and_arbitrary_directory_even_forced() {
    let repo = Repo::new("remove-safety");
    let parent = repo.linked("parent", "registered-parent");
    let child = repo.linked("child", "registered-parent/registered-child");
    let locked = repo.linked("locked", "locked");
    git(
        &repo.main,
        &[
            "worktree",
            "lock",
            "--reason",
            "protected",
            "--",
            text(&locked),
        ],
    );
    let arbitrary = repo.parent.join("arbitrary");
    write_file(&arbitrary.join("sentinel"), b"not a worktree\0");
    let service = GitService::new();
    for force in [false, true] {
        for path in [&repo.main, &locked, &parent, &arbitrary] {
            assert_invalid(service.worktree(
                &repo.main,
                GitWorktreeOp::Remove {
                    worktree_path: text(path).into(),
                    force,
                },
                &BTreeMap::new(),
            ));
            assert!(path.is_dir());
        }
        assert_eq!(fs::read(child.join("a.txt")).unwrap(), b"one\n");
        assert_eq!(
            fs::read(arbitrary.join("sentinel")).unwrap(),
            b"not a worktree\0"
        );
        assert_eq!(
            service.list_worktrees(&repo.main).unwrap().worktrees.len(),
            4
        );
    }
}

#[test]
fn ignored_only_worktree_requires_explicit_force_to_remove() {
    let repo = Repo::new("ignored-remove");
    let linked = repo.linked("ignored-only", "ignored-only");
    let ignored = linked.join("ignored/cache.bin");
    write_file(&ignored, b"ignored bytes\0\xff");
    assert!(git(&linked, &["status", "--porcelain=v1", "-z"]).is_empty());
    let service = GitService::new();
    let error = service
        .worktree(
            &repo.main,
            GitWorktreeOp::Remove {
                worktree_path: text(&linked).into(),
                force: false,
            },
            &BTreeMap::new(),
        )
        .unwrap_err();
    assert!(matches!(error, ApiError::WorktreeHasChanges { path, .. } if path == text(&linked)));
    assert_eq!(fs::read(&ignored).unwrap(), b"ignored bytes\0\xff");
    assert!(matches!(
        service
            .worktree(
                &repo.main,
                GitWorktreeOp::Remove {
                    worktree_path: text(&linked).into(),
                    force: true
                },
                &BTreeMap::new(),
            )
            .unwrap(),
        GitWorktreeResult::Removed
    ));
    assert!(!linked.exists());
    assert_eq!(
        service.list_worktrees(&repo.main).unwrap().worktrees.len(),
        1
    );
}

#[test]
fn prune_removes_stale_registration_but_preserves_locked_and_live_worktrees() {
    let repo = Repo::new("prune");
    let stale = repo.linked("stale", "stale");
    let locked = repo.linked("locked", "locked-stale");
    let live = repo.linked("live", "live");
    git(
        &repo.main,
        &[
            "worktree",
            "lock",
            "--reason",
            "removable drive",
            "--",
            text(&locked),
        ],
    );
    fs::remove_dir_all(&stale).unwrap();
    fs::remove_dir_all(&locked).unwrap();
    let service = GitService::new();
    assert!(row(&service.list_worktrees(&repo.main).unwrap(), &stale)
        .prunable
        .is_some());
    assert!(matches!(
        service
            .worktree(&repo.main, GitWorktreeOp::Prune, &BTreeMap::new())
            .unwrap(),
        GitWorktreeResult::Pruned
    ));
    let list = service.list_worktrees(&repo.main).unwrap();
    assert_eq!(list.worktrees.len(), 3);
    assert!(!list.worktrees.iter().any(|row| row.path == text(&stale)));
    assert_eq!(
        row(&list, &locked).locked.as_deref(),
        Some("removable drive")
    );
    assert!(row(&list, &live).prunable.is_none());
    assert_eq!(fs::read(live.join("a.txt")).unwrap(), b"one\n");
    assert!(repo.main.is_dir());
    assert!(!locked.exists());
}

#[test]
fn migration_preserves_index_and_worktree_bytes_moves_untracked_and_retains_recovery() {
    let repo = Repo::new("migration");
    let source = repo.linked("source", "source");
    let old_stash = prior_stash(&repo.main);
    let old_stash_object = git(&repo.main, &["cat-file", "commit", &old_stash]);
    let head = commit_oid(&source, "HEAD").unwrap();
    let staged = b"staged bytes\0\xff\n";
    let unstaged = b"unstaged bytes\0\xfe\n";
    write_file(&source.join("a.txt"), staged);
    git(&source, &["add", "a.txt"]);
    write_file(&source.join("a.txt"), unstaged);
    let untracked_name = "new dir/untracked\tline\n.bin";
    let untracked = b"untracked\0\xff\n";
    write_file(&source.join(untracked_name), untracked);
    let ignored = b"ignored stays here\0\xff\n";
    write_file(&source.join("ignored/cache.bin"), ignored);
    let before = diffs(&source);
    assert!(!before.0.is_empty() && !before.1.is_empty());
    assert_ne!(before.0, before.1);
    assert_eq!(git(&source, &["show", ":a.txt"]), staged);
    let service = GitService::new();
    let result = service
        .worktree(
            &source,
            GitWorktreeOp::Migrate {
                worktree_path: text(&source).into(),
            },
            &BTreeMap::new(),
        )
        .unwrap();
    let recovery = match result {
        GitWorktreeResult::Migrated {
            main_path,
            recovery_stash: Some(oid),
        } => {
            assert_eq!(main_path, text(&repo.main));
            oid
        }
        other => panic!("Expected migration with recovery stash, got {other:?}"),
    };
    assert_eq!(commit_oid(&source, "HEAD").unwrap(), head);
    assert_eq!(commit_oid(&repo.main, "HEAD").unwrap(), head);
    assert_eq!(diffs(&repo.main), before);
    assert_eq!(git(&repo.main, &["show", ":a.txt"]), staged);
    assert_eq!(fs::read(repo.main.join("a.txt")).unwrap(), unstaged);
    assert_eq!(fs::read(repo.main.join(untracked_name)).unwrap(), untracked);
    assert!(!source.join(untracked_name).exists());
    assert_eq!(diffs(&source), (Vec::new(), Vec::new()));
    assert_eq!(git(&source, &["show", ":a.txt"]), b"one\n");
    assert_eq!(fs::read(source.join("a.txt")).unwrap(), b"one\n");
    assert_eq!(fs::read(source.join("ignored/cache.bin")).unwrap(), ignored);
    assert!(!repo.main.join("ignored/cache.bin").exists());
    assert_recovery_stash(&repo.main, &recovery);
    assert_eq!(
        git(&repo.main, &["show", &format!("{recovery}^2:a.txt")]),
        staged
    );
    assert_eq!(
        git(&repo.main, &["show", &format!("{recovery}:a.txt")]),
        unstaged
    );
    assert_eq!(
        git(
            &repo.main,
            &["show", &format!("{recovery}^3:{untracked_name}")]
        ),
        untracked
    );
    assert_eq!(
        line(&repo.main, &["reflog", "show", "--format=%H", "refs/stash"]),
        format!("{recovery}\n{old_stash}")
    );
    assert_eq!(
        recovery_oid(&repo.main, "preexisting stash")
            .unwrap()
            .as_deref(),
        Some(old_stash.as_str())
    );
    assert_eq!(
        git(&repo.main, &["cat-file", "commit", &old_stash]),
        old_stash_object
    );
    assert_eq!(
        git(&repo.main, &["show", &format!("{old_stash}:a.txt")]),
        b"preexisting stash bytes\n"
    );
    assert_eq!(
        service.list_worktrees(&repo.main).unwrap().worktrees.len(),
        2
    );
}

#[test]
fn migration_dirty_main_aborts_before_stashing_or_changing_source() {
    let repo = Repo::new("dirty-main");
    let source = repo.linked("source", "source");
    let stash = prior_stash(&repo.main);
    write_file(&source.join("a.txt"), b"source staged\n");
    git(&source, &["add", "a.txt"]);
    write_file(&source.join("a.txt"), b"source unstaged\n");
    write_file(&source.join("untracked.bin"), b"source untracked\0");
    write_file(&repo.main.join("b.txt"), b"dirty main\n");
    let source_diffs = diffs(&source);
    let main_diffs = diffs(&repo.main);
    let reflog = git(
        &repo.main,
        &["reflog", "show", "--format=%H%x00%gs", "-z", "refs/stash"],
    );
    let error = GitService::new()
        .worktree(
            &repo.main,
            GitWorktreeOp::Migrate {
                worktree_path: text(&source).into(),
            },
            &BTreeMap::new(),
        )
        .unwrap_err();
    assert!(matches!(error, ApiError::DirtyWorktree { .. }));
    assert_eq!(diffs(&source), source_diffs);
    assert_eq!(diffs(&repo.main), main_diffs);
    assert_eq!(git(&source, &["show", ":a.txt"]), b"source staged\n");
    assert_eq!(
        fs::read(source.join("a.txt")).unwrap(),
        b"source unstaged\n"
    );
    assert_eq!(
        fs::read(source.join("untracked.bin")).unwrap(),
        b"source untracked\0"
    );
    assert_eq!(fs::read(repo.main.join("b.txt")).unwrap(), b"dirty main\n");
    assert_eq!(commit_oid(&repo.main, "refs/stash").unwrap(), stash);
    assert_eq!(
        git(
            &repo.main,
            &["reflog", "show", "--format=%H%x00%gs", "-z", "refs/stash"]
        ),
        reflog
    );
}

#[test]
fn migration_conflict_keeps_unmerged_main_restores_source_and_retains_recovery() {
    let repo = Repo::new("migration-conflict");
    let source = repo.linked("source", "source");
    let old_stash = prior_stash(&repo.main);
    write_file(&source.join("a.txt"), b"source committed\n");
    git(&source, &["commit", "-q", "-am", "source diverges"]);
    write_file(&repo.main.join("a.txt"), b"main committed\n");
    git(&repo.main, &["commit", "-q", "-am", "main diverges"]);
    let source_head = commit_oid(&source, "HEAD").unwrap();
    let main_head = commit_oid(&repo.main, "HEAD").unwrap();
    assert_ne!(source_head, main_head);
    // The conflicting path is unstaged: a conflicting cached patch can fail
    // before Git's merge runs and would not exercise retained unmerged state.
    write_file(&source.join("a.txt"), b"source unstaged conflict\n");
    write_file(&source.join("b.txt"), b"source staged nonconflict\n");
    git(&source, &["add", "b.txt"]);
    write_file(
        &source.join("untracked.bin"),
        b"untracked survives restore\0\xff",
    );
    let before = diffs(&source);
    let service = GitService::new();
    let error = service
        .worktree(
            &repo.main,
            GitWorktreeOp::Migrate {
                worktree_path: text(&source).into(),
            },
            &BTreeMap::new(),
        )
        .unwrap_err();
    let message = match error {
        ApiError::MergeConflicts { message } => message,
        other => panic!("Expected actual merge conflicts, got {other:?}"),
    };
    let recovery = commit_oid(&repo.main, "refs/stash").unwrap();
    let label = assert_recovery_stash(&repo.main, &recovery);
    assert!(message.contains(text(&source)) && message.contains(text(&repo.main)));
    assert!(message.contains(&recovery) && message.contains(&label));
    assert!(message.contains("Source changes restored with staging preserved"));
    assert_eq!(commit_oid(&source, "HEAD").unwrap(), source_head);
    assert_eq!(commit_oid(&repo.main, "HEAD").unwrap(), main_head);
    assert_eq!(diffs(&source), before);
    assert_eq!(
        fs::read(source.join("a.txt")).unwrap(),
        b"source unstaged conflict\n"
    );
    assert_eq!(git(&source, &["show", ":a.txt"]), b"source committed\n");
    assert_eq!(
        git(&source, &["show", ":b.txt"]),
        b"source staged nonconflict\n"
    );
    assert_eq!(
        fs::read(source.join("untracked.bin")).unwrap(),
        b"untracked survives restore\0\xff"
    );
    assert!(git(&source, &["ls-files", "--unmerged", "-z"]).is_empty());
    assert_eq!(
        git(
            &repo.main,
            &["diff", "--name-only", "--diff-filter=U", "-z", "--"]
        ),
        b"a.txt\0"
    );
    assert_eq!(
        git(&repo.main, &["show", ":1:a.txt"]),
        b"source committed\n"
    );
    assert_eq!(git(&repo.main, &["show", ":2:a.txt"]), b"main committed\n");
    assert_eq!(
        git(&repo.main, &["show", ":3:a.txt"]),
        b"source unstaged conflict\n"
    );
    let main_bytes = fs::read(repo.main.join("a.txt")).unwrap();
    assert!(main_bytes
        .windows(b"<<<<<<<".len())
        .any(|window| window == b"<<<<<<<"));
    assert_eq!(
        git(&repo.main, &["show", &format!("{recovery}^2:b.txt")]),
        b"source staged nonconflict\n"
    );
    assert_eq!(
        git(&repo.main, &["show", &format!("{recovery}:a.txt")]),
        b"source unstaged conflict\n"
    );
    assert_eq!(
        line(&repo.main, &["reflog", "show", "--format=%H", "refs/stash"]),
        format!("{recovery}\n{old_stash}")
    );
    assert_eq!(
        recovery_oid(&repo.main, "preexisting stash")
            .unwrap()
            .as_deref(),
        Some(old_stash.as_str())
    );
}

#[test]
fn source_restore_refuses_changed_head_without_overwriting_clean_checkout() {
    let repo = Repo::new("restore-head");
    let source = repo.linked("source", "source");
    let captured_head = commit_oid(&source, "HEAD").unwrap();
    write_file(&source.join("a.txt"), b"captured source edit\n");
    git(&source, &["stash", "push", "-q", "-m", "guarded capture"]);
    let recovery = commit_oid(&source, "refs/stash").unwrap();
    write_file(&source.join("b.txt"), b"new source commit\n");
    git(&source, &["commit", "-q", "-am", "external head change"]);
    let changed_head = commit_oid(&source, "HEAD").unwrap();
    assert_ne!(changed_head, captured_head);
    assert_eq!(diffs(&source), (Vec::new(), Vec::new()));
    assert_invalid(GitService::new().restore_migration_source(&source, &captured_head, &recovery));
    assert_eq!(commit_oid(&source, "HEAD").unwrap(), changed_head);
    assert_eq!(diffs(&source), (Vec::new(), Vec::new()));
    assert_eq!(fs::read(source.join("a.txt")).unwrap(), b"one\n");
    assert_eq!(
        fs::read(source.join("b.txt")).unwrap(),
        b"new source commit\n"
    );
    assert_eq!(git(&source, &["show", ":b.txt"]), b"new source commit\n");
    assert_eq!(commit_oid(&source, "refs/stash").unwrap(), recovery);
    assert_eq!(
        git(&source, &["show", &format!("{recovery}:a.txt")]),
        b"captured source edit\n"
    );
}

#[test]
fn source_restore_refuses_changed_bytes_without_overwriting_index_or_worktree() {
    let repo = Repo::new("restore-bytes");
    let source = repo.linked("source", "source");
    let head = commit_oid(&source, "HEAD").unwrap();
    write_file(&source.join("a.txt"), b"captured source edit\n");
    git(&source, &["stash", "push", "-q", "-m", "guarded capture"]);
    let recovery = commit_oid(&source, "refs/stash").unwrap();
    write_file(&source.join("a.txt"), b"external unstaged bytes\0\xff");
    write_file(&source.join("b.txt"), b"external staged bytes\0\xfe");
    git(&source, &["add", "b.txt"]);
    write_file(
        &source.join("external-untracked.bin"),
        b"external untracked bytes\0",
    );
    let before = diffs(&source);
    assert_invalid(GitService::new().restore_migration_source(&source, &head, &recovery));
    assert_eq!(commit_oid(&source, "HEAD").unwrap(), head);
    assert_eq!(diffs(&source), before);
    assert_eq!(
        fs::read(source.join("a.txt")).unwrap(),
        b"external unstaged bytes\0\xff"
    );
    assert_eq!(git(&source, &["show", ":a.txt"]), b"one\n");
    assert_eq!(
        git(&source, &["show", ":b.txt"]),
        b"external staged bytes\0\xfe"
    );
    assert_eq!(
        fs::read(source.join("b.txt")).unwrap(),
        b"external staged bytes\0\xfe"
    );
    assert_eq!(
        fs::read(source.join("external-untracked.bin")).unwrap(),
        b"external untracked bytes\0"
    );
    assert!(git(&source, &["ls-files", "--unmerged", "-z"]).is_empty());
    assert_eq!(commit_oid(&source, "refs/stash").unwrap(), recovery);
}

#[test]
fn locked_clean_source_migrates_without_stash_but_cannot_be_force_removed() {
    let repo = Repo::new("locked-migrate");
    let source = repo.linked("locked-source", "locked-source");
    let old_stash = prior_stash(&repo.main);
    git(
        &repo.main,
        &[
            "worktree",
            "lock",
            "--reason",
            "keep registration",
            "--",
            text(&source),
        ],
    );
    let service = GitService::new();
    let result = service
        .worktree(
            &source,
            GitWorktreeOp::Migrate {
                worktree_path: text(&source).into(),
            },
            &BTreeMap::new(),
        )
        .unwrap();
    match result {
        GitWorktreeResult::Migrated {
            main_path,
            recovery_stash: None,
        } => {
            assert_eq!(main_path, text(&repo.main));
        }
        other => panic!("Expected clean migration without stash, got {other:?}"),
    }
    assert_eq!(commit_oid(&repo.main, "refs/stash").unwrap(), old_stash);
    assert_eq!(diffs(&source), (Vec::new(), Vec::new()));
    assert_eq!(diffs(&repo.main), (Vec::new(), Vec::new()));
    assert_invalid(service.worktree(
        &repo.main,
        GitWorktreeOp::Remove {
            worktree_path: text(&source).into(),
            force: true,
        },
        &BTreeMap::new(),
    ));
    assert_eq!(fs::read(source.join("a.txt")).unwrap(), b"one\n");
    assert_eq!(
        row(&service.list_worktrees(&repo.main).unwrap(), &source)
            .locked
            .as_deref(),
        Some("keep registration")
    );
}

#[test]
fn migration_ignored_destination_collision_preserves_target_and_restores_source() {
    let repo = Repo::new("migration-ignored-collision");
    let source = repo.linked("source", "source");
    let old_stash = prior_stash(&repo.main);
    let target_bytes = b"ignored target must survive\0\xff";
    let source_bytes = b"nonignored source must survive\0\xfe";
    write_file(&repo.main.join("ignored/cache.bin"), target_bytes);
    // Removing the source ignore rule makes its same-named file capturable;
    // the primary checkout still ignores and owns its existing bytes.
    write_file(&source.join(".gitignore"), b"");
    git(&source, &["add", ".gitignore"]);
    write_file(&source.join("ignored/cache.bin"), source_bytes);
    assert_eq!(
        git(
            &source,
            &["ls-files", "--others", "--exclude-standard", "-z"]
        ),
        b"ignored/cache.bin\0"
    );
    assert_eq!(
        git(
            &repo.main,
            &[
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "-z"
            ]
        ),
        b"ignored/cache.bin\0"
    );
    let before = diffs(&source);
    let source_head = commit_oid(&source, "HEAD").unwrap();
    let service = GitService::new();
    let error = service
        .worktree(
            &repo.main,
            GitWorktreeOp::Migrate {
                worktree_path: text(&source).into(),
            },
            &BTreeMap::new(),
        )
        .unwrap_err();
    let message = match error {
        ApiError::InvalidArgument(message) => message,
        other => panic!("Expected ignored collision refusal, got {other:?}"),
    };
    let recovery = commit_oid(&repo.main, "refs/stash").unwrap();
    assert_recovery_stash(&repo.main, &recovery);
    assert!(message.contains("Ignored destination file"));
    assert!(message.contains(&recovery));
    assert!(message.contains(text(&source)) && message.contains(text(&repo.main)));
    assert_eq!(
        fs::read(repo.main.join("ignored/cache.bin")).unwrap(),
        target_bytes
    );
    assert_eq!(
        fs::read(repo.main.join(".gitignore")).unwrap(),
        b"ignored/\n"
    );
    assert_eq!(diffs(&repo.main), (Vec::new(), Vec::new()));
    assert!(git(&repo.main, &["ls-files", "--unmerged", "-z"]).is_empty());
    assert_eq!(commit_oid(&source, "HEAD").unwrap(), source_head);
    assert_eq!(diffs(&source), before);
    assert_eq!(git(&source, &["show", ":.gitignore"]), b"");
    assert_eq!(fs::read(source.join(".gitignore")).unwrap(), b"");
    assert_eq!(
        fs::read(source.join("ignored/cache.bin")).unwrap(),
        source_bytes
    );
    assert_eq!(
        git(
            &repo.main,
            &["show", &format!("{recovery}^3:ignored/cache.bin")]
        ),
        source_bytes
    );
    assert_eq!(
        line(&repo.main, &["reflog", "show", "--format=%H", "refs/stash"]),
        format!("{recovery}\n{old_stash}")
    );
}

#[test]
fn migration_without_untracked_files_succeeds_when_main_has_ignored_files() {
    let repo = Repo::new("migration-tracked-only");
    let source = repo.linked("source", "source");
    write_file(&repo.main.join("ignored/cache.bin"), b"main ignored\n");
    write_file(&source.join("a.txt"), b"tracked edit\n");
    let before = diffs(&source);
    let service = GitService::new();
    write_file(&source.join("ignored/cache.bin"), b"source ignored\n");
    git(&source, &["stash", "push", "-q", "-m", "tracked-only"]);
    let tracked_stash = commit_oid(&source, "refs/stash").unwrap();
    assert!(commit_oid(&source, &format!("{tracked_stash}^3")).is_err());
    ensure_no_ignored_collisions(&repo.main, &tracked_stash).unwrap();
    service
        .restore_migration_source(
            &source,
            &commit_oid(&source, "HEAD").unwrap(),
            &tracked_stash,
        )
        .unwrap();
    assert_eq!(diffs(&source), before);
    let result = service
        .worktree(
            &source,
            GitWorktreeOp::Migrate {
                worktree_path: text(&source).into(),
            },
            &BTreeMap::new(),
        )
        .unwrap();
    let recovery = match result {
        GitWorktreeResult::Migrated {
            recovery_stash: Some(oid),
            ..
        } => oid,
        other => panic!("Expected migration with recovery stash, got {other:?}"),
    };
    assert_recovery_stash(&repo.main, &recovery);
    assert_eq!(diffs(&repo.main), before);
    assert_eq!(
        fs::read(repo.main.join("ignored/cache.bin")).unwrap(),
        b"main ignored\n"
    );
    assert_eq!(diffs(&source), (Vec::new(), Vec::new()));
}

#[test]
fn post_add_copy_failure_keeps_registered_checkout_and_new_branch() {
    let repo = Repo::new("copy-failure");
    let candidate = repo.main.join("setup.txt");
    write_file(&candidate, b"selected setup bytes\n");
    let hook = repo.parent.join("empty-hooks/post-checkout");
    // This generated /tmp path contains only the fixed fixture name and UUID.
    write_file(
        &hook,
        format!("#!/bin/sh\nrm -- '{}'\n", text(&candidate)).as_bytes(),
    );
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let destination = repo.parent.join("created-checkout");
    let service = GitService::new();
    let result = service
        .worktree(
            &repo.main,
            GitWorktreeOp::Add {
                destination: text(&destination).into(),
                checkout: GitWorktreeCheckout::NewBranch {
                    name: "copy-failed-but-created".into(),
                    base: "main".into(),
                },
            },
            &BTreeMap::from([("setup.txt".into(), true)]),
        )
        .unwrap();
    match result {
        GitWorktreeResult::Added {
            path,
            copy_complete: false,
            warnings,
        } => {
            assert_eq!(path, text(&destination));
            assert!(!warnings.is_empty());
            assert!(warnings.iter().any(|warning| warning.contains("setup.txt")));
        }
        other => panic!("Expected successful checkout with failed copy, got {other:?}"),
    }
    assert!(!candidate.exists());
    assert!(!destination.join("setup.txt").exists());
    assert!(destination.is_dir());
    let head = commit_oid(&repo.main, "HEAD").unwrap();
    assert_eq!(commit_oid(&destination, "HEAD").unwrap(), head);
    assert_eq!(
        commit_oid(&repo.main, "refs/heads/copy-failed-but-created").unwrap(),
        head
    );
    let list = service.list_worktrees(&repo.main).unwrap();
    assert_eq!(list.worktrees.len(), 2);
    assert_eq!(
        row(&list, &destination).branch.as_deref(),
        Some("copy-failed-but-created")
    );
    assert_eq!(fs::read(destination.join("a.txt")).unwrap(), b"one\n");
}

#[test]
fn migration_dirty_nested_repository_aborts_before_capturing_source() {
    let repo = Repo::new("dirty-nested");
    let source = repo.linked("source", "source");
    let old_stash = prior_stash(&repo.main);
    let nested = source.join("nested");
    fs::create_dir(&nested).unwrap();
    git(&nested, &["init", "-q", "-b", "main"]);
    git(&nested, &["config", "user.name", "Nested Test"]);
    git(&nested, &["config", "user.email", "nested@test.invalid"]);
    git(&nested, &["config", "commit.gpgsign", "false"]);
    git(
        &nested,
        &[
            "config",
            "core.hooksPath",
            text(&repo.parent.join("empty-hooks")),
        ],
    );
    write_file(&nested.join("tracked.txt"), b"nested committed\n");
    git(&nested, &["add", "tracked.txt"]);
    git(&nested, &["commit", "-q", "-m", "nested initial"]);
    write_file(&nested.join("tracked.txt"), b"nested dirty bytes\n");
    write_file(&source.join("a.txt"), b"source staged\n");
    git(&source, &["add", "a.txt"]);
    let before = diffs(&source);
    let nested_before = diffs(&nested);
    assert_invalid(GitService::new().worktree(
        &source,
        GitWorktreeOp::Migrate {
            worktree_path: text(&source).into(),
        },
        &BTreeMap::new(),
    ));
    assert_eq!(diffs(&source), before);
    assert_eq!(diffs(&nested), nested_before);
    assert_eq!(
        fs::read(nested.join("tracked.txt")).unwrap(),
        b"nested dirty bytes\n"
    );
    assert_eq!(commit_oid(&repo.main, "refs/stash").unwrap(), old_stash);
    assert_eq!(diffs(&repo.main), (Vec::new(), Vec::new()));
}

#[test]
fn migration_refuses_dirty_submodule_even_when_parent_status_ignores_it() {
    let repo = Repo::new("dirty-submodule");
    let module_repo = repo.parent.join("module-origin");
    fs::create_dir(&module_repo).unwrap();
    git(&module_repo, &["init", "-q", "-b", "main"]);
    git(&module_repo, &["config", "user.name", "Submodule Test"]);
    git(
        &module_repo,
        &["config", "user.email", "module@test.invalid"],
    );
    git(&module_repo, &["config", "commit.gpgsign", "false"]);
    git(
        &module_repo,
        &[
            "config",
            "core.hooksPath",
            text(&repo.parent.join("empty-hooks")),
        ],
    );
    write_file(&module_repo.join("tracked.txt"), b"module committed\n");
    git(&module_repo, &["add", "tracked.txt"]);
    git(&module_repo, &["commit", "-q", "-m", "module initial"]);
    git(
        &repo.main,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            text(&module_repo),
            "module",
        ],
    );
    git(&repo.main, &["commit", "-q", "-am", "register submodule"]);
    let source = repo.linked("source", "source");
    git(
        &source,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "update",
            "--init",
        ],
    );
    git(&source, &["config", "submodule.module.ignore", "all"]);
    let old_stash = prior_stash(&repo.main);
    write_file(
        &source.join("module/tracked.txt"),
        b"dirty submodule bytes\n",
    );
    assert!(git(&source, &["status", "--porcelain=v1", "-z"]).is_empty());
    let source_head = commit_oid(&source, "HEAD").unwrap();
    assert_invalid(GitService::new().worktree(
        &source,
        GitWorktreeOp::Migrate {
            worktree_path: text(&source).into(),
        },
        &BTreeMap::new(),
    ));
    assert_eq!(commit_oid(&source, "HEAD").unwrap(), source_head);
    assert_eq!(
        fs::read(source.join("module/tracked.txt")).unwrap(),
        b"dirty submodule bytes\n"
    );
    assert_eq!(git(&source, &["show", ":a.txt"]), b"one\n");
    assert_eq!(commit_oid(&repo.main, "refs/stash").unwrap(), old_stash);
    assert_eq!(
        fs::read(repo.main.join("module/tracked.txt")).unwrap(),
        b"module committed\n"
    );
}
