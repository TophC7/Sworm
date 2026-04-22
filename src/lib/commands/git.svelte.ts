import type { CommandConfirm, CommandGroup } from './types'
import { getActiveProject } from '$lib/stores/projects.svelte'
import { getGitSummary, runGitAction } from '$lib/stores/git.svelte'
import { getActiveProjectId } from '$lib/workbench/state.svelte'
import { backend } from '$lib/api/backend'
import { getGitActionNotifications, type GitActionKind } from '$lib/utils/gitActionNotifications'
import { runNotifiedTask } from '$lib/utils/notifiedTask'

import {
  ArrowDownToLineIcon,
  ArrowUpFromLineIcon,
  RefreshCwIcon,
  ShieldAlertIcon,
  Undo2Icon
} from '$lib/icons/lucideExports'

let undoConfirmOpen = $state(false)

const undoConfirm: CommandConfirm = {
  title: 'Undo Last Commit?',
  message: 'This will soft-reset to HEAD~1. Your changes will be preserved as staged files.',
  confirmLabel: 'Undo Commit',
  isOpen: () => undoConfirmOpen,
  onConfirm: () => {
    undoConfirmOpen = false
    void doGitAction('undoLastCommit', (path) => backend.git.undoLastCommit(path))
  },
  onCancel: () => {
    undoConfirmOpen = false
  }
}

async function doGitAction(kind: GitActionKind, fn: (path: string) => Promise<unknown>) {
  const project = getActiveProject()
  if (!project) return
  await runNotifiedTask(() => runGitAction(project.id, project.path, fn), getGitActionNotifications(kind))
}

export function getGitCommands(): CommandGroup[] {
  const activeId = getActiveProjectId()
  if (!activeId) return []

  const gitSummary = getGitSummary(activeId)
  const hasCommits = !!gitSummary?.branch

  return [
    {
      heading: 'Git',
      commands: [
        {
          id: 'git-pull',
          label: 'Pull',
          icon: ArrowDownToLineIcon,
          keywords: ['git', 'pull', 'download', 'sync'],
          onSelect: () => void doGitAction('pull', (path) => backend.git.pull(path))
        },
        {
          id: 'git-push',
          label: 'Push',
          icon: ArrowUpFromLineIcon,
          keywords: ['git', 'push', 'upload', 'sync'],
          onSelect: () => void doGitAction('push', (path) => backend.git.push(path))
        },
        {
          id: 'git-fetch',
          label: 'Fetch',
          icon: RefreshCwIcon,
          keywords: ['git', 'fetch', 'remote', 'update'],
          onSelect: () => void doGitAction('fetch', (path) => backend.git.fetch(path))
        },
        {
          id: 'git-force-push',
          label: 'Force Push (with lease)',
          icon: ShieldAlertIcon,
          keywords: ['git', 'force', 'push', 'lease'],
          onSelect: () => void doGitAction('forcePush', (path) => backend.git.pushForceWithLease(path))
        },
        ...(hasCommits
          ? [
              {
                id: 'git-undo-last-commit',
                label: 'Undo Last Commit',
                icon: Undo2Icon,
                keywords: ['git', 'undo', 'reset', 'revert', 'commit'],
                onSelect: () => {
                  undoConfirmOpen = true
                },
                confirm: undoConfirm
              }
            ]
          : [])
      ]
    }
  ]
}
