import test, { describe } from 'node:test'
import assert from 'node:assert/strict'
import type { GitChange, GitOperationKind, GitSummary } from '$lib/types/backend'
import { resolvePrimaryAction, type PostCommit } from './commitButton'

const change: GitChange = {
  path: 'file.txt',
  status: 'M',
  staged: true,
  additions: 1,
  deletions: 1
}

function summary(patch: Partial<GitSummary> = {}): GitSummary {
  return {
    is_repo: true,
    branch: 'main',
    base_ref: 'HEAD',
    ahead: 0,
    behind: 0,
    changes: [],
    staged_count: 0,
    unstaged_count: 0,
    untracked_count: 0,
    upstream: 'origin/main',
    detached: false,
    unborn: false,
    head: null,
    operation: null,
    conflicts: [],
    message_template: null,
    ...patch
  }
}

const operations: [GitOperationKind, string][] = [
  ['merge', 'Continue Merge'],
  ['rebase', 'Continue Rebase'],
  ['cherry_pick', 'Continue Cherry-pick'],
  ['revert', 'Continue Revert']
]

const ordinaryCandidates: Partial<GitSummary>[] = [
  { changes: [change], staged_count: 1, upstream: null, ahead: 2 },
  { upstream: null, ahead: 2 },
  { ahead: 2, behind: 1 },
  {}
]

describe('resolvePrimaryAction', () => {
  for (const [kind, title] of operations) {
    for (const conflicted of [false, true]) {
      test(`${kind} ${conflicted ? 'blocks' : 'enables'} Continue and overrides ordinary actions`, () => {
        for (const candidate of ordinaryCandidates) {
          const paused = summary({
            ...candidate,
            operation: { kind, target: null, headName: null, step: null, total: null, commit: null },
            conflicts: conflicted ? [{ path: 'conflict.txt', kind: 'both_modified' }] : []
          })
          assert.deepEqual(resolvePrimaryAction(paused, '', 'push'), {
            kind: 'continue',
            title,
            enabled: !conflicted
          })
        }
      })
    }
  }

  test('continues a detached rebase without a draft or working-tree changes', () => {
    const paused = summary({
      branch: null,
      detached: true,
      operation: { kind: 'rebase', target: 'main', headName: 'feature', step: 1, total: 2, commit: null }
    })
    assert.deepEqual(resolvePrimaryAction(paused, '', 'sync'), {
      kind: 'continue',
      title: 'Continue Rebase',
      enabled: true
    })
  })

  test('changes keep Commit ahead of Publish and Sync, even without a usable message', () => {
    for (const upstream of [null, 'origin/main']) {
      const changed = summary({ changes: [change], staged_count: 1, upstream, ahead: 2, behind: 1 })
      assert.deepEqual(resolvePrimaryAction(changed, '  \n ', 'push'), {
        kind: 'commit',
        post: 'push',
        enabled: false
      })
      assert.deepEqual(resolvePrimaryAction(changed, '  Fix file  ', 'sync'), {
        kind: 'commit',
        post: 'sync',
        enabled: true
      })
    }
  })

  test('Publish takes precedence over Sync when an existing branch lacks an upstream', () => {
    assert.deepEqual(resolvePrimaryAction(summary({ upstream: null, ahead: 2, behind: 1 }), '', 'none'), {
      kind: 'publish',
      branch: 'main'
    })
  })

  test('Sync uses both counts and treats unavailable counts as zero', () => {
    for (const [ahead, behind] of [
      [2, 1],
      [2, null],
      [null, 1]
    ]) {
      assert.deepEqual(resolvePrimaryAction(summary({ ahead, behind }), '', 'none'), {
        kind: 'sync',
        ahead: ahead ?? 0,
        behind: behind ?? 0
      })
    }
  })

  test('a clean branch falls back to disabled Commit regardless of draft and post action', () => {
    for (const post of ['none', 'push', 'sync'] as PostCommit[]) {
      assert.deepEqual(resolvePrimaryAction(summary(), 'Unused draft', post), {
        kind: 'commit',
        post,
        enabled: false
      })
    }
  })

  test('detached HEAD cannot Publish, Sync, or run a post-commit branch action', () => {
    const detached = summary({ branch: null, detached: true, upstream: null, ahead: 2, behind: 1 })
    assert.deepEqual(resolvePrimaryAction(detached, 'Fix file', 'push'), {
      kind: 'commit',
      post: 'none',
      enabled: false
    })
    assert.deepEqual(resolvePrimaryAction({ ...detached, changes: [change] }, 'Fix file', 'sync'), {
      kind: 'commit',
      post: 'none',
      enabled: true
    })
  })

  test('an unborn branch offers Commit rather than Publish or Sync', () => {
    const unborn = summary({ unborn: true, upstream: null, ahead: 2 })
    assert.deepEqual(resolvePrimaryAction(unborn, 'Initial commit', 'none'), {
      kind: 'commit',
      post: 'none',
      enabled: false
    })
    assert.deepEqual(resolvePrimaryAction({ ...unborn, changes: [change] }, 'Initial commit', 'none'), {
      kind: 'commit',
      post: 'none',
      enabled: true
    })
  })
})
