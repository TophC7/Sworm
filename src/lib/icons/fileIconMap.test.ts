import test, { describe } from 'node:test'
import assert from 'node:assert/strict'
import { resolveFileIcon } from './fileIconMap'

describe('fileIconMap', () => {
  test('resolves legacy .sworm/tasks.json to plain json icon (not sworm)', () => {
    assert.equal(resolveFileIcon('.sworm/tasks.json'), 'json')
    assert.equal(resolveFileIcon('/repo/.sworm/tasks.json'), 'json')
  })

  test('resolves .sworm/tasks.jsonc and tasks.jsonc to sworm icon', () => {
    assert.equal(resolveFileIcon('.sworm/tasks.jsonc'), 'sworm')
    assert.equal(resolveFileIcon('/repo/.sworm/tasks.jsonc'), 'sworm')
    assert.equal(resolveFileIcon('tasks.jsonc'), 'sworm')
  })

  test('resolves .sworm/settings.jsonc and settings.jsonc to sworm icon', () => {
    assert.equal(resolveFileIcon('.sworm/settings.jsonc'), 'sworm')
    assert.equal(resolveFileIcon('/repo/.sworm/settings.jsonc'), 'sworm')
    assert.equal(resolveFileIcon('/home/user/.config/sworm/settings.jsonc'), 'sworm')
    assert.equal(resolveFileIcon('settings.jsonc'), 'sworm')
  })

  test('preserves vscode icon for .vscode/tasks.json', () => {
    assert.equal(resolveFileIcon('.vscode/tasks.json'), 'vscode')
    assert.equal(resolveFileIcon('tasks.json'), 'vscode')
  })

  test('resolves generic jsonc files to json icon', () => {
    assert.equal(resolveFileIcon('other.jsonc'), 'json')
  })
})
