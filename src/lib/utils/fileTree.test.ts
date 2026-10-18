import test from 'node:test'
import assert from 'node:assert/strict'
import { buildFileTree, findTreeRowPath } from './fileTree'

function rows(paths: string[], cases: Record<string, string | null>): void {
  const tree = buildFileTree(paths.map((path) => ({ path })))
  for (const [path, row] of Object.entries(cases)) assert.equal(findTreeRowPath(tree, path), row, path)
}

test('findTreeRowPath maps a path to the row that shows it, including compacted chains', () => {
  rows(['src/lib/a.ts', 'src/lib/b.ts'], {
    src: 'src/lib',
    'src/lib': 'src/lib',
    'src/lib/a.ts': 'src/lib/a.ts',
    sr: null
  })
  rows(['a/b/c/f.txt', 'a/x.txt'], {
    'a/b': 'a/b/c',
    a: 'a',
    'a/x.txt': 'a/x.txt',
    other: null
  })
})
