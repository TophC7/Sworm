import type { GitStatusKind } from '$lib/types/backend'

interface GitStatusMeta {
  color: string
  letter: string
  label: string
}

const KIND_STATUS: Record<GitStatusKind, GitStatusMeta> = {
  added: { color: 'text-success', letter: 'A', label: 'Added' },
  modified: { color: 'text-accent', letter: 'M', label: 'Modified' },
  deleted: { color: 'text-danger', letter: 'D', label: 'Deleted' },
  renamed: { color: 'text-accent', letter: 'R', label: 'Renamed' },
  copied: { color: 'text-accent', letter: 'C', label: 'Copied' },
  untracked: { color: 'text-success', letter: '?', label: 'Untracked' },
  unmerged: { color: 'text-warning', letter: 'U', label: 'Unmerged' },
  unknown: { color: 'text-muted', letter: ' ', label: 'Unknown' }
}

export function gitStatusMeta(status: string): GitStatusMeta {
  if (status in KIND_STATUS) {
    return KIND_STATUS[status as GitStatusKind]
  }
  switch (status) {
    case 'A':
      return KIND_STATUS.added
    case 'D':
      return KIND_STATUS.deleted
    case 'R':
      return KIND_STATUS.renamed
    case 'M':
      return { color: 'text-warning', letter: 'M', label: 'Modified' }
    case '?':
      return { color: 'text-success', letter: 'U', label: 'Untracked' }
    default:
      return { color: 'text-muted', letter: status, label: 'Changed' }
  }
}

