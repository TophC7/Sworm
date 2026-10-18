import type { GitOperation } from '$lib/types/backend'

export interface OperationDisplay {
  /** Short noun for notifications. */
  noun: string
  /** Capitalized noun for actions. */
  title: string
  /** Progress or target headline for the paused strip. */
  headline: string
  /** Replayed commit detail, if any. */
  detail: string | null
  /** Short status-bar chip suffix. */
  chipSuffix: string
  /** Merge has no commit to skip. */
  canSkip: boolean
}

export function describeOperation(operation: GitOperation): OperationDisplay {
  const detail = operation.commit ? `${operation.commit.shortHash} ${operation.commit.subject}` : null
  switch (operation.kind) {
    case 'merge':
      return {
        noun: 'merge',
        title: 'Merge',
        headline: operation.target ? `Merging ${operation.target}` : 'Merging in progress',
        detail: null,
        chipSuffix: '(Merging)',
        canSkip: false
      }
    case 'rebase': {
      const stepSuffix = operation.step && operation.total ? ` (${operation.step}/${operation.total})` : ''
      return {
        noun: 'rebase',
        title: 'Rebase',
        headline: `Rebasing ${operation.headName ?? 'HEAD'} onto ${operation.target ?? 'upstream'}${stepSuffix}`,
        detail,
        chipSuffix: '(Rebasing)',
        canSkip: true
      }
    }
    case 'cherry_pick':
      return {
        noun: 'cherry-pick',
        title: 'Cherry-pick',
        headline: 'Cherry-pick in progress',
        detail,
        chipSuffix: '(Cherry-picking)',
        canSkip: true
      }
    case 'revert':
      return {
        noun: 'revert',
        title: 'Revert',
        headline: 'Revert in progress',
        detail,
        chipSuffix: '(Reverting)',
        canSkip: true
      }
  }
}
