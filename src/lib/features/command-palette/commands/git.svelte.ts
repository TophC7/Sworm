import type { AppCommandDefinition } from './registry.svelte'
import { backend } from '$lib/api/backend'
import { pickAsync } from '$lib/features/confirm/service.svelte'
import * as actions from '$lib/features/git/actions.svelte'
import * as branches from '$lib/features/git/branches.svelte'
import { remoteActionState } from '$lib/features/git/commitButton'
import { openGitDialog } from '$lib/features/git/dialogs/host.svelte'
import { describeOperation } from '$lib/features/git/operation'
import { getGitSummary } from '$lib/features/git/state.svelte'
import { confirmDiscardAll } from '$lib/features/git/treeActions'
import { notify } from '$lib/features/notifications/state.svelte'
import { getActiveFolderPath } from '$lib/features/workbench/state.svelte'
import { openWorkingTreeDiff } from '$lib/features/workbench/surfaces/diff/service.svelte'
import type { GitSummary, GitWorktree } from '$lib/types/backend'
import { getErrorMessage } from '$lib/utils/client-error'
import {
  ArrowDownToLineIcon,
  ArrowRight,
  ArrowUpFromLineIcon,
  Check,
  CloudIcon,
  CloudUploadIcon,
  FileDiff,
  GitBranchPlusIcon,
  GitCommitIcon,
  GitMergeIcon,
  GitPullRequestIcon,
  ListTreeIcon,
  MinusCircle,
  PackageIcon,
  PackageOpenIcon,
  PencilIcon,
  PlusCircle,
  RefreshCwIcon,
  RotateCw,
  ShieldAlertIcon,
  TagIcon,
  Trash2,
  Undo2Icon,
  X
} from '$lib/icons/lucideExports'

function activeRepo(): { folderPath: string; summary: GitSummary } | null {
  const folderPath = getActiveFolderPath()
  const summary = folderPath !== null ? getGitSummary(folderPath) : null
  return folderPath !== null && summary?.is_repo ? { folderPath, summary } : null
}

function gitCommand(
  definition: Omit<AppCommandDefinition, 'group' | 'visible' | 'run'> & {
    when?: (summary: GitSummary) => boolean
    run: (folderPath: string, summary: GitSummary) => unknown
  }
): AppCommandDefinition {
  const { when, run, ...command } = definition
  return {
    ...command,
    group: 'Git',
    visible: () => {
      const repo = activeRepo()
      return repo !== null && (when?.(repo.summary) ?? true)
    },
    run: () => {
      const repo = activeRepo()
      if (repo && (when?.(repo.summary) ?? true)) return Promise.resolve(run(repo.folderPath, repo.summary))
    }
  }
}

const tracking = (summary: GitSummary) => remoteActionState(summary).tracking
const publishable = (summary: GitSummary) => remoteActionState(summary).publishable
const idle = (summary: GitSummary) => summary.operation === null
const born = (summary: GitSummary) => !summary.unborn
const attached = (summary: GitSummary) => summary.branch !== null && !summary.detached && !summary.unborn

function opTitle(): string {
  const operation = activeRepo()?.summary.operation
  return operation ? describeOperation(operation).title : 'Operation'
}

async function pickStash(folderPath: string, title: string): Promise<number | null> {
  const stashes = await backend.git.stashList(folderPath).catch((error) => {
    notify.error('Load stashes failed', getErrorMessage(error))
    return null
  })
  if (stashes === null) return null
  if (stashes.length === 0) {
    notify.info('No stashes', 'There are no stashes in the repository.')
    return null
  }
  const value = await pickAsync({
    title,
    filterPlaceholder: 'Select a stash',
    options: stashes.map((stash) => ({
      value: String(stash.index),
      label: `stash@{${stash.index}}`,
      description: stash.message
    }))
  })
  return value === null ? null : Number(value)
}

async function pickTag(folderPath: string, title: string): Promise<string | null> {
  const tags = await backend.git.listTags(folderPath).catch((error) => {
    notify.error('Load tags failed', getErrorMessage(error))
    return null
  })
  if (tags === null) return null
  if (tags.length === 0) {
    notify.info('No tags', 'This repository has no tags.')
    return null
  }
  return pickAsync({
    title,
    filterPlaceholder: 'Select a tag',
    options: tags.map((tag) => ({
      value: tag.name,
      label: tag.name,
      description: `${tag.annotated ? 'Annotated' : 'Lightweight'} · ${tag.shortHash}`
    }))
  })
}

async function pickWorktree(
  folderPath: string,
  title: string,
  eligible: (worktree: GitWorktree) => boolean
): Promise<string | null> {
  const inventory = await backend.git.listWorktrees(folderPath).catch((error) => {
    notify.error('Load worktrees failed', getErrorMessage(error))
    return null
  })
  if (inventory === null) return null
  const worktrees = inventory.worktrees.filter(eligible)
  if (worktrees.length === 0) {
    notify.info('No linked worktrees', 'This repository has no linked worktree for this action.')
    return null
  }
  return pickAsync({
    title,
    filterPlaceholder: 'Select a worktree',
    options: worktrees.map((worktree) => ({
      value: worktree.path,
      label: worktree.branch ?? `Detached ${worktree.head?.slice(0, 8) ?? 'HEAD'}`,
      description: worktree.path
    }))
  })
}

export function getGitCommandDefinitions(): AppCommandDefinition[] {
  return [
    gitCommand({
      id: 'git-continue',
      label: () => `Continue ${opTitle()}`,
      icon: Check,
      keywords: ['git', 'continue', 'merge', 'rebase', 'cherry-pick', 'revert'],
      when: (summary) => summary.operation !== null && summary.conflicts.length === 0,
      run: (folderPath) => actions.continueOperation(folderPath)
    }),
    gitCommand({
      id: 'git-skip',
      label: 'Skip Commit',
      icon: ArrowRight,
      keywords: ['git', 'skip', 'commit', 'rebase', 'cherry-pick', 'revert'],
      when: (summary) => summary.operation !== null && describeOperation(summary.operation).canSkip,
      run: (folderPath) => actions.skipOperation(folderPath)
    }),
    gitCommand({
      id: 'git-abort',
      label: () => `Abort ${opTitle()}`,
      icon: X,
      keywords: ['git', 'abort', 'cancel', 'merge', 'rebase', 'cherry-pick', 'revert'],
      when: (summary) => summary.operation !== null,
      run: (folderPath) => actions.abortOperation(folderPath)
    }),
    gitCommand({
      id: 'git-sync',
      label: 'Sync',
      icon: RefreshCwIcon,
      keywords: ['git', 'sync', 'pull', 'push'],
      when: tracking,
      run: (folderPath) => actions.syncFolder(folderPath)
    }),
    gitCommand({
      id: 'git-sync-rebase',
      label: 'Sync (Rebase)',
      icon: RefreshCwIcon,
      keywords: ['git', 'sync', 'rebase', 'pull', 'push'],
      when: tracking,
      run: (folderPath) => actions.syncFolder(folderPath, true)
    }),
    gitCommand({
      id: 'git-pull',
      label: 'Pull',
      icon: ArrowDownToLineIcon,
      keywords: ['git', 'pull', 'download', 'sync'],
      when: tracking,
      run: (folderPath) => actions.pullFolder(folderPath)
    }),
    gitCommand({
      id: 'git-pull-rebase',
      label: 'Pull (Rebase)',
      icon: ArrowDownToLineIcon,
      keywords: ['git', 'pull', 'rebase', 'download', 'sync'],
      when: tracking,
      run: (folderPath) => actions.pullFolder(folderPath, true)
    }),
    gitCommand({
      id: 'git-push',
      label: 'Push',
      icon: ArrowUpFromLineIcon,
      keywords: ['git', 'push', 'upload', 'sync'],
      when: tracking,
      run: (folderPath) => actions.pushFolder(folderPath)
    }),
    gitCommand({
      id: 'git-force-push',
      label: 'Force Push (with lease)',
      icon: ShieldAlertIcon,
      keywords: ['git', 'force', 'push', 'lease'],
      dangerous: true,
      when: tracking,
      run: (folderPath) => actions.forcePushWithLease(folderPath)
    }),
    gitCommand({
      id: 'git-publish',
      label: 'Publish Branch…',
      icon: CloudUploadIcon,
      keywords: ['git', 'publish', 'branch', 'remote', 'upstream', 'push'],
      when: publishable,
      run: (folderPath, summary) => actions.publishBranch(folderPath, summary.branch)
    }),
    gitCommand({
      id: 'git-fetch',
      label: 'Fetch',
      icon: RotateCw,
      keywords: ['git', 'fetch', 'remote', 'update'],
      run: (folderPath) => actions.fetchFolder(folderPath)
    }),
    gitCommand({
      id: 'git-commit',
      label: 'Commit',
      icon: GitCommitIcon,
      keywords: ['git', 'commit', 'message', 'save'],
      when: (summary) => idle(summary) && summary.changes.length > 0,
      run: (folderPath) => actions.commitWithPrompt(folderPath, { kind: 'commit', post: 'none' })
    }),
    gitCommand({
      id: 'git-commit-all',
      label: 'Commit All',
      icon: GitCommitIcon,
      keywords: ['git', 'commit', 'all', 'stage', 'save'],
      when: (summary) => idle(summary) && summary.changes.length > 0,
      run: (folderPath) => actions.commitWithPrompt(folderPath, { kind: 'all' })
    }),
    gitCommand({
      id: 'git-commit-push',
      label: 'Commit & Push',
      icon: ArrowUpFromLineIcon,
      keywords: ['git', 'commit', 'push', 'upload'],
      when: (summary) => idle(summary) && summary.changes.length > 0 && summary.branch !== null && !summary.detached,
      run: (folderPath) => actions.commitWithPrompt(folderPath, { kind: 'commit', post: 'push' })
    }),
    gitCommand({
      id: 'git-commit-sync',
      label: 'Commit & Sync',
      icon: RefreshCwIcon,
      keywords: ['git', 'commit', 'sync', 'pull', 'push'],
      when: (summary) => idle(summary) && summary.changes.length > 0 && summary.branch !== null && !summary.detached,
      run: (folderPath) => actions.commitWithPrompt(folderPath, { kind: 'commit', post: 'sync' })
    }),
    gitCommand({
      id: 'git-commit-amend',
      label: 'Commit (Amend)',
      icon: GitCommitIcon,
      keywords: ['git', 'commit', 'amend', 'rewrite', 'message'],
      when: (summary) => idle(summary) && born(summary),
      run: (folderPath) => actions.commitWithPrompt(folderPath, { kind: 'amend' })
    }),
    gitCommand({
      id: 'git-undo-last-commit',
      label: 'Undo Last Commit',
      icon: Undo2Icon,
      keywords: ['git', 'undo', 'reset', 'revert', 'commit'],
      dangerous: true,
      when: (summary) => idle(summary) && born(summary),
      run: (folderPath) => actions.undoLastCommit(folderPath)
    }),
    gitCommand({
      id: 'git-stage-all',
      label: 'Stage All Changes',
      icon: PlusCircle,
      keywords: ['git', 'stage', 'all', 'changes', 'add'],
      when: (summary) => summary.unstaged_count + summary.untracked_count > 0,
      run: (folderPath) => actions.stageAllFolder(folderPath)
    }),
    gitCommand({
      id: 'git-unstage-all',
      label: 'Unstage All Changes',
      icon: MinusCircle,
      keywords: ['git', 'unstage', 'all', 'changes', 'index'],
      when: (summary) => summary.staged_count > 0,
      run: (folderPath) => actions.unstageAllFolder(folderPath)
    }),
    gitCommand({
      id: 'git-discard-all',
      label: 'Discard All Changes',
      icon: Trash2,
      keywords: ['git', 'discard', 'all', 'changes', 'reset', 'clean'],
      dangerous: true,
      when: (summary) => summary.unstaged_count + summary.untracked_count > 0,
      run: (folderPath) => confirmDiscardAll(folderPath)
    }),
    gitCommand({
      id: 'git-open-changes',
      label: 'Open Changes',
      icon: FileDiff,
      keywords: ['git', 'open', 'changes', 'diff', 'working', 'tree'],
      when: (summary) => summary.unstaged_count + summary.untracked_count > 0,
      run: (folderPath) => openWorkingTreeDiff(folderPath, false, null, null, { temporary: false })
    }),
    gitCommand({
      id: 'git-checkout-to',
      label: 'Checkout to…',
      icon: Check,
      keywords: ['git', 'checkout', 'switch', 'branch', 'ref'],
      when: idle,
      run: (folderPath) => branches.checkoutTo(folderPath)
    }),
    gitCommand({
      id: 'git-checkout-detached',
      label: 'Checkout to (Detached)…',
      icon: GitCommitIcon,
      keywords: ['git', 'checkout', 'detached', 'head', 'ref'],
      when: idle,
      run: async (folderPath) => {
        const ref = await branches.pickRef(folderPath, {
          title: 'Checkout to (Detached)',
          placeholder: 'Select a ref to checkout in detached mode',
          errorTitle: 'Checkout failed',
          includeCurrent: true
        })
        if (ref) await branches.checkoutRef(folderPath, { kind: 'detached', rev: ref.name, label: ref.name })
      }
    }),
    gitCommand({
      id: 'git-create-branch',
      label: 'Create Branch…',
      icon: GitBranchPlusIcon,
      keywords: ['git', 'create', 'branch', 'new'],
      when: born,
      run: (folderPath, summary) => openGitDialog({ kind: 'create_branch', folderPath, base: summary.branch ?? 'HEAD' })
    }),
    gitCommand({
      id: 'git-create-branch-from',
      label: 'Create Branch From…',
      icon: GitBranchPlusIcon,
      keywords: ['git', 'create', 'branch', 'from', 'ref', 'new'],
      when: born,
      run: async (folderPath) => {
        const ref = await branches.pickRef(folderPath, {
          title: 'Create Branch From',
          placeholder: 'Select a ref to create the branch from',
          errorTitle: 'Create branch failed',
          includeCurrent: true
        })
        if (ref) openGitDialog({ kind: 'create_branch', folderPath, base: ref.name })
      }
    }),
    gitCommand({
      id: 'git-rename-branch',
      label: 'Rename Branch…',
      icon: PencilIcon,
      keywords: ['git', 'rename', 'branch'],
      when: (summary) => summary.branch !== null && !summary.detached,
      run: (folderPath, summary) => openGitDialog({ kind: 'rename_branch', folderPath, branch: summary.branch! })
    }),
    gitCommand({
      id: 'git-delete-branch',
      label: 'Delete Branch…',
      icon: Trash2,
      keywords: ['git', 'delete', 'branch', 'remove'],
      dangerous: true,
      run: async (folderPath) => {
        const ref = await branches.pickRef(folderPath, {
          title: 'Delete Branch',
          placeholder: 'Select a branch to delete',
          errorTitle: 'Delete branch failed',
          remotes: false,
          tags: false
        })
        if (!ref) return
        const branch = branches.byFolder
          .get(folderPath)
          ?.list.find((entry) => entry.kind === 'local' && entry.name === ref.name)
        if (branch) openGitDialog({ kind: 'delete_branch', folderPath, branch })
      }
    }),
    gitCommand({
      id: 'git-merge',
      label: 'Merge…',
      icon: GitMergeIcon,
      keywords: ['git', 'merge', 'branch', 'tag'],
      when: (summary) => attached(summary) && idle(summary),
      run: async (folderPath, summary) => {
        const ref = await branches.pickRef(folderPath, {
          title: 'Merge',
          placeholder: `Select a branch or tag to merge into ${summary.branch}`,
          errorTitle: 'Merge failed'
        })
        if (ref) openGitDialog({ kind: 'merge', folderPath, source: ref.name, current: summary.branch! })
      }
    }),
    gitCommand({
      id: 'git-rebase',
      label: 'Rebase Branch…',
      icon: GitPullRequestIcon,
      keywords: ['git', 'rebase', 'branch', 'onto', 'rewrite'],
      when: (summary) => attached(summary) && idle(summary),
      run: async (folderPath, summary) => {
        const ref = await branches.pickRef(folderPath, {
          title: 'Rebase Branch',
          placeholder: `Select a branch to rebase ${summary.branch} onto`,
          errorTitle: 'Rebase failed',
          tags: false
        })
        if (ref) await actions.rebaseCurrentOnto(folderPath, ref.name)
      }
    }),
    gitCommand({
      id: 'git-delete-merged-branches',
      label: 'Delete Merged Branches…',
      icon: Trash2,
      keywords: ['git', 'delete', 'merged', 'branches', 'cleanup'],
      dangerous: true,
      when: born,
      run: (folderPath) => actions.deleteMergedBranches(folderPath)
    }),
    gitCommand({
      id: 'git-stash',
      label: 'Stash…',
      icon: PackageIcon,
      keywords: ['git', 'stash', 'save', 'changes'],
      when: (summary) => summary.staged_count + summary.unstaged_count > 0,
      run: (folderPath) => actions.promptStash(folderPath, { kind: 'all', includeUntracked: false })
    }),
    gitCommand({
      id: 'git-stash-untracked',
      label: 'Stash (Include Untracked)…',
      icon: PackageIcon,
      keywords: ['git', 'stash', 'include', 'untracked', 'all', 'save'],
      when: (summary) => summary.changes.length > 0,
      run: (folderPath) => actions.promptStash(folderPath, { kind: 'all', includeUntracked: true })
    }),
    gitCommand({
      id: 'git-stash-staged',
      label: 'Stash Staged…',
      icon: PackageIcon,
      keywords: ['git', 'stash', 'staged', 'index', 'save'],
      when: (summary) => summary.staged_count > 0,
      run: (folderPath) => actions.promptStash(folderPath, { kind: 'staged' })
    }),
    gitCommand({
      id: 'git-stash-apply-latest',
      label: 'Apply Latest Stash',
      icon: PackageOpenIcon,
      keywords: ['git', 'apply', 'latest', 'stash', 'restore'],
      run: (folderPath) => actions.applyLatestStash(folderPath)
    }),
    gitCommand({
      id: 'git-stash-pop-latest',
      label: 'Pop Latest Stash',
      icon: PackageOpenIcon,
      keywords: ['git', 'pop', 'latest', 'stash', 'restore'],
      run: (folderPath) => actions.popLatestStash(folderPath)
    }),
    gitCommand({
      id: 'git-stash-apply',
      label: 'Apply Stash…',
      icon: PackageOpenIcon,
      keywords: ['git', 'apply', 'stash', 'restore'],
      run: async (folderPath) => {
        const index = await pickStash(folderPath, 'Apply Stash')
        if (index !== null) await actions.applyStash(folderPath, index)
      }
    }),
    gitCommand({
      id: 'git-stash-pop',
      label: 'Pop Stash…',
      icon: PackageOpenIcon,
      keywords: ['git', 'pop', 'stash', 'restore'],
      run: async (folderPath) => {
        const index = await pickStash(folderPath, 'Pop Stash')
        if (index !== null) await actions.popStash(folderPath, index)
      }
    }),
    gitCommand({
      id: 'git-stash-drop',
      label: 'Drop Stash…',
      icon: Trash2,
      keywords: ['git', 'drop', 'stash', 'delete', 'remove'],
      dangerous: true,
      run: async (folderPath) => {
        const index = await pickStash(folderPath, 'Drop Stash')
        if (index !== null) await actions.dropStash(folderPath, index)
      }
    }),
    gitCommand({
      id: 'git-stash-drop-all',
      label: 'Drop All Stashes…',
      icon: Trash2,
      keywords: ['git', 'drop', 'all', 'stashes', 'clear', 'delete'],
      dangerous: true,
      run: (folderPath) => actions.clearStashes(folderPath)
    }),
    gitCommand({
      id: 'git-create-tag',
      label: 'Create Tag…',
      icon: TagIcon,
      keywords: ['git', 'create', 'tag', 'new', 'release'],
      when: born,
      run: (folderPath) => actions.createTag(folderPath, 'HEAD', 'HEAD')
    }),
    gitCommand({
      id: 'git-delete-tag',
      label: 'Delete Tag…',
      icon: TagIcon,
      keywords: ['git', 'delete', 'tag', 'remove'],
      dangerous: true,
      run: async (folderPath) => {
        const name = await pickTag(folderPath, 'Delete Tag')
        if (name !== null) await actions.deleteTag(folderPath, name)
      }
    }),
    gitCommand({
      id: 'git-push-tags',
      label: 'Push All Tags…',
      icon: CloudUploadIcon,
      keywords: ['git', 'push', 'all', 'tags', 'remote', 'publish'],
      run: (folderPath) => actions.pushTag(folderPath, null)
    }),
    gitCommand({
      id: 'git-delete-remote-tag',
      label: 'Delete Remote Tag…',
      icon: TagIcon,
      keywords: ['git', 'delete', 'remote', 'tag', 'remove'],
      dangerous: true,
      run: async (folderPath) => {
        const name = await pickTag(folderPath, 'Delete Remote Tag')
        if (name !== null) await actions.deleteRemoteTag(folderPath, name)
      }
    }),
    gitCommand({
      id: 'git-add-remote',
      label: 'Add Remote…',
      icon: CloudIcon,
      keywords: ['git', 'add', 'remote', 'url', 'origin'],
      run: (folderPath) => actions.addRemote(folderPath)
    }),
    gitCommand({
      id: 'git-rename-remote',
      label: 'Rename Remote…',
      icon: PencilIcon,
      keywords: ['git', 'rename', 'remote'],
      run: (folderPath) => actions.renameRemote(folderPath)
    }),
    gitCommand({
      id: 'git-set-remote-url',
      label: 'Set Remote URL…',
      icon: CloudIcon,
      keywords: ['git', 'set', 'remote', 'url', 'change'],
      run: (folderPath) => actions.setRemoteUrl(folderPath)
    }),
    gitCommand({
      id: 'git-remove-remote',
      label: 'Remove Remote…',
      icon: Trash2,
      keywords: ['git', 'remove', 'remote', 'delete'],
      dangerous: true,
      run: (folderPath) => actions.removeRemote(folderPath)
    }),
    gitCommand({
      id: 'git-create-worktree',
      label: 'Create Worktree…',
      icon: ListTreeIcon,
      keywords: ['git', 'create', 'worktree', 'linked', 'new'],
      when: born,
      run: (folderPath) => openGitDialog({ kind: 'create_worktree', folderPath })
    }),
    gitCommand({
      id: 'git-remove-worktree',
      label: 'Remove Worktree…',
      icon: Trash2,
      keywords: ['git', 'remove', 'worktree', 'linked', 'delete'],
      dangerous: true,
      run: async (folderPath) => {
        const path = await pickWorktree(
          folderPath,
          'Remove Worktree',
          (worktree) => !worktree.isMain && !worktree.bare && worktree.prunable === null && worktree.locked === null
        )
        if (path !== null) await actions.removeWorktree(folderPath, path)
      }
    }),
    gitCommand({
      id: 'git-migrate-worktree',
      label: 'Migrate Worktree Changes to Main…',
      icon: GitMergeIcon,
      keywords: ['git', 'migrate', 'worktree', 'changes', 'main', 'move'],
      dangerous: true,
      run: async (folderPath) => {
        const path = await pickWorktree(
          folderPath,
          'Migrate Worktree Changes',
          (worktree) => !worktree.isMain && !worktree.bare && worktree.prunable === null
        )
        if (path !== null) await actions.migrateWorktreeChanges(folderPath, path)
      }
    }),
    gitCommand({
      id: 'git-prune-worktrees',
      label: 'Prune Stale Worktrees',
      icon: Trash2,
      keywords: ['git', 'prune', 'stale', 'worktrees', 'cleanup'],
      run: (folderPath) => actions.pruneWorktrees(folderPath)
    })
  ]
}
