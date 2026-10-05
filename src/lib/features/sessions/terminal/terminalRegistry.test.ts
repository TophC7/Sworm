import test from 'node:test'
import assert from 'node:assert/strict'
import type { TerminalTransferState } from '$lib/types/backend'
import { createTerminalRegistry } from './terminalRegistry'

test('registry keeps detach, release, dispose and import rollback distinct', async () => {
  const calls: string[] = []
  const state: TerminalTransferState = {
    runId: 'run',
    serializedBuffer: 'history',
    cols: 80,
    rows: 24,
    viewportPosition: 0,
    lastSequence: 1
  }
  const failure = new Error('attach failed')
  const registry = createTerminalRegistry('terminal', (key: string) => key, (key) => {
    calls.push(`create:${key}`)
    return {
      detach: () => { calls.push(`detach:${key}`) },
      release: () => { calls.push(`release:${key}`) },
      dispose: () => { calls.push(`dispose:${key}`) },
      focus: () => { calls.push(`focus:${key}`) },
      exportTransferState: async () => state,
      importTransferState: async (received: TerminalTransferState, transferId: string) => {
        assert.equal(received, state)
        assert.equal(transferId, 'transfer')
        if (key === 'failed') throw failure
      }
    }
  })

  const first = registry.getOrCreate('first')
  assert.equal(registry.getOrCreate('first'), first)
  registry.detach('first')
  registry.focus('first')
  assert.equal(registry.get('first'), first)
  assert.equal(await registry.exportTransferState('first'), state)
  registry.release('first')
  assert.equal(registry.get('first'), undefined)
  registry.release('first')

  registry.getOrCreate('closed')
  registry.dispose('closed')
  assert.equal(registry.get('closed'), undefined)
  await registry.importTransferState('imported', state, 'transfer')
  assert.ok(registry.get('imported'))
  await assert.rejects(registry.importTransferState('failed', state, 'transfer'), (error) => error === failure)
  assert.equal(registry.get('failed'), undefined)
  await assert.rejects(registry.exportTransferState('failed'), /Unknown terminal failed/)
  registry.getOrCreate('last')
  registry.releaseAll()
  assert.equal(registry.get('imported'), undefined)
  assert.equal(registry.get('last'), undefined)
  assert.deepEqual(calls, [
    'create:first', 'detach:first', 'focus:first', 'release:first',
    'create:closed', 'dispose:closed', 'create:imported',
    'create:failed', 'release:failed', 'create:last', 'release:imported', 'release:last'
  ])
})
