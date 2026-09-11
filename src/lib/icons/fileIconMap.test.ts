import test, { describe } from 'node:test'
import assert from 'node:assert/strict'
import { resolveFileIcon } from './fileIconMap'

describe('fileIconMap', () => {
  test('resolves legacy .sworm/tasks.json to plain json icon (not sworm)', () => {
    assert.equal(resolveFileIcon('.sworm/tasks.json'), '/icons/bearded/json.svg')
    assert.equal(resolveFileIcon('/repo/.sworm/tasks.json'), '/icons/bearded/json.svg')
  })

  test('resolves .sworm/tasks.jsonc and tasks.jsonc to sworm icon', () => {
    assert.equal(resolveFileIcon('.sworm/tasks.jsonc'), '/icons/bearded/sworm.svg')
    assert.equal(resolveFileIcon('/repo/.sworm/tasks.jsonc'), '/icons/bearded/sworm.svg')
    assert.equal(resolveFileIcon('tasks.jsonc'), '/icons/bearded/sworm.svg')
  })

  test('resolves .sworm/settings.jsonc and settings.jsonc to sworm icon', () => {
    assert.equal(resolveFileIcon('.sworm/settings.jsonc'), '/icons/bearded/sworm.svg')
    assert.equal(resolveFileIcon('/repo/.sworm/settings.jsonc'), '/icons/bearded/sworm.svg')
    assert.equal(resolveFileIcon('/home/user/.config/sworm/settings.jsonc'), '/icons/bearded/sworm.svg')
    assert.equal(resolveFileIcon('settings.jsonc'), '/icons/bearded/sworm.svg')
  })

  test('preserves vscode icon for .vscode/tasks.json', () => {
    assert.equal(resolveFileIcon('.vscode/tasks.json'), '/icons/bearded/vscode.svg')
    assert.equal(resolveFileIcon('tasks.json'), '/icons/bearded/vscode.svg')
  })

  test('resolves generic jsonc files to json icon', () => {
    assert.equal(resolveFileIcon('other.jsonc'), '/icons/bearded/json.svg')
  })
})
