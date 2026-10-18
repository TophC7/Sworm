import type { GitConflictKind, GitStatusKind } from '$lib/types/backend'

interface GitStatusMeta {
  color: string
  letter: string
  label: string
}

type StatusKind = GitStatusKind | GitConflictKind | 'conflict'

const KIND_STATUS: Record<StatusKind, GitStatusMeta> = {
  added: { color: 'text-success', letter: 'A', label: 'Added' },
  modified: { color: 'text-accent', letter: 'M', label: 'Modified' },
  deleted: { color: 'text-danger', letter: 'D', label: 'Deleted' },
  renamed: { color: 'text-accent', letter: 'R', label: 'Renamed' },
  copied: { color: 'text-accent', letter: 'C', label: 'Copied' },
  untracked: { color: 'text-success', letter: '?', label: 'Untracked' },
  unmerged: { color: 'text-warning', letter: 'U', label: 'Unmerged' },
  conflict: { color: 'text-warning', letter: '!', label: 'Conflict' },
  both_modified: { color: 'text-warning', letter: '!', label: 'Conflict: Both Modified' },
  both_added: { color: 'text-warning', letter: '!', label: 'Conflict: Both Added' },
  both_deleted: { color: 'text-warning', letter: '!', label: 'Conflict: Both Deleted' },
  added_by_us: { color: 'text-warning', letter: '!', label: 'Conflict: Added By Us' },
  added_by_them: { color: 'text-warning', letter: '!', label: 'Conflict: Added By Them' },
  deleted_by_us: { color: 'text-warning', letter: '!', label: 'Conflict: Deleted By Us' },
  deleted_by_them: { color: 'text-warning', letter: '!', label: 'Conflict: Deleted By Them' },
  unknown: { color: 'text-muted', letter: ' ', label: 'Unknown' }
}

export function gitStatusMeta(status: string): GitStatusMeta {
  if (status in KIND_STATUS) {
    return KIND_STATUS[status as StatusKind]
  }
  switch (status) {
    case 'A':
      return KIND_STATUS.added
    case 'D':
      return KIND_STATUS.deleted
    case 'R':
      return KIND_STATUS.renamed
    case 'U':
      return KIND_STATUS.unmerged
    case 'M':
      return { color: 'text-warning', letter: 'M', label: 'Modified' }
    case '?':
      return { color: 'text-success', letter: 'U', label: 'Untracked' }
    default:
      return { color: 'text-muted', letter: status, label: 'Changed' }
  }
}
