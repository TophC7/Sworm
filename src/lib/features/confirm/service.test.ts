import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { compileModule } from 'svelte/compiler'
import ts from 'typescript'
import type * as ConfirmService from './service.svelte'

// Compile runes as Svelte does; server output lets queue logic run without a DOM.
const source = readFileSync(new URL('./service.svelte.ts', import.meta.url), 'utf8')
const javascript = ts.transpileModule(source, {
  compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext }
}).outputText
const compiled = compileModule(javascript, { filename: 'service.svelte.js', generate: 'server' }).js.code
const code = compiled.replace(/(['"])(svelte\/[^'"]+)\1/g, (_match, _quote, specifier: string) =>
  JSON.stringify(import.meta.resolve(specifier))
)
// Static import cannot execute runes; load the runtime-compiled module instead.
const { confirmAsync, getPendingConfirm, resolvePendingConfirm } = (await import(
  `data:text/javascript;base64,${Buffer.from(code).toString('base64')}`
)) as typeof ConfirmService

test('confirmation queue fences stale callbacks, busy repeats, cancellation, and failures', async () => {
  let runs = 0
  const gate = Promise.withResolvers<void>()
  const first = confirmAsync({
    title: 'First',
    message: 'First',
    run: () => {
      runs++
      return gate.promise
    }
  })
  const second = confirmAsync({ title: 'Second', message: 'Second' })
  const firstId = getPendingConfirm()!.id
  assert.equal(getPendingConfirm()!.busy, false)
  await resolvePendingConfirm(firstId + 1, false)
  assert.equal(getPendingConfirm()!.id, firstId)

  const completing = resolvePendingConfirm(firstId, true)
  assert.equal(getPendingConfirm()!.busy, true)
  await resolvePendingConfirm(firstId, true)
  await resolvePendingConfirm(firstId, false)
  assert.equal(runs, 1)
  assert.equal(getPendingConfirm()!.id, firstId)
  gate.resolve()
  await completing
  assert.equal(await first, true)

  const secondId = getPendingConfirm()!.id
  await resolvePendingConfirm(firstId, false)
  assert.equal(getPendingConfirm()!.id, secondId)
  await resolvePendingConfirm(secondId, true)
  assert.equal(await second, true)

  const cancelled = confirmAsync({ title: 'Cancel', message: 'Cancel', run: async () => { runs++ } })
  await resolvePendingConfirm(getPendingConfirm()!.id, false)
  assert.equal(await cancelled, false)
  assert.equal(runs, 1)

  const failure = new Error('Delete failed')
  const failed = confirmAsync({ title: 'Fail', message: 'Fail', run: async () => { throw failure } })
  const rejection = assert.rejects(failed, (error) => error === failure)
  const afterFailure = confirmAsync({ title: 'After', message: 'After' })
  const failedId = getPendingConfirm()!.id
  await resolvePendingConfirm(failedId, true)
  await rejection
  await resolvePendingConfirm(failedId, false)
  assert.notEqual(getPendingConfirm()!.id, failedId)
  await resolvePendingConfirm(getPendingConfirm()!.id, false)
  assert.equal(await afterFailure, false)
  assert.equal(getPendingConfirm(), null)
})
