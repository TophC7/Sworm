import test, { describe } from 'node:test'
import assert from 'node:assert/strict'
import type { GitChange, GitSummary } from '$lib/types/backend'
import { pathGitState } from './gitStatus'

const change: GitChange = {
  path: 'f',
  status: 'M',
  staged: false,
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

describe('pathGitState', () => {
  test('folder scope includes nested changes but excludes sibling prefixes', () => {
    const state = pathGitState(
      summary({
        changes: [
          { ...change, path: 'dir/a.txt' },
          { ...change, path: 'dir2/x.txt' },
          { ...change, path: 'dir/sub/b.txt', status: '?' }
        ]
      }),
      'dir',
      'directory'
    )
    assert.deepEqual(state, {
      staged: [],
      unstaged: ['dir/a.txt', 'dir/sub/b.txt'],
      inHead: false
    })
  })

  test('a conflict-only folder remains revealable without actionable changed paths', () => {
    const state = pathGitState(
      summary({
        changes: [
          { ...change, path: 'dir/c.txt', staged: true },
          { ...change, path: 'dir/c.txt' }
        ],
        conflicts: [{ path: 'dir/c.txt', kind: 'both_modified' }]
      }),
      'dir',
      'directory'
    )
    assert.deepEqual(state, { staged: [], unstaged: [], inHead: false })
  })

  test('a clean file has no git state', () => {
    assert.equal(pathGitState(summary(), 'clean.txt', 'file'), null)
  })

  test('a staged-added file modified again does not exist at HEAD', () => {
    const state = pathGitState(
      summary({
        changes: [{ ...change, status: 'A', staged: true }, change]
      }),
      'f',
      'file'
    )
    assert.equal(state?.inHead, false)
  })

  test('a tracked file can have both staged and unstaged changes and exist at HEAD', () => {
    const state = pathGitState(
      summary({
        changes: [{ ...change, staged: true }, change]
      }),
      'f',
      'file'
    )
    assert.deepEqual(state, { staged: ['f'], unstaged: ['f'], inHead: true })
  })
})
