import test, { describe } from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { FileAssembly } from './transport.web.files'

// 0x00, 0xff, and an invalid UTF-8 sequence (0xc3 0x28) must survive untouched.
const binary = new Uint8Array([0x00, 0xff, 0xc3, 0x28, 0x41])
const binaryHash = createHash('sha256').update(binary).digest('hex')

describe('FileAssembly', () => {
  test('returns binary bytes unchanged after digest verification', async () => {
    const assembly = new FileAssembly(binary.length)
    assembly.append(binary.subarray(0, 2))
    assembly.append(binary.subarray(2))
    const result = await assembly.complete(binaryHash)
    assert.equal(result.version, binaryHash)
    assert.deepEqual(Array.from(result.bytes), Array.from(binary))
  })

  test('rejects truncated, oversized, and hash-mismatched content', async () => {
    const truncated = new FileAssembly(binary.length)
    truncated.append(binary.subarray(0, 3))
    await assert.rejects(truncated.complete(binaryHash), /File read size mismatch/)

    const oversized = new FileAssembly(2)
    assert.throws(() => oversized.append(binary), /File read size mismatch/)

    const mismatched = new FileAssembly(binary.length)
    mismatched.append(binary)
    await assert.rejects(mismatched.complete('0'.repeat(64)), /File read hash mismatch/)
  })

  test('cancellation during a pending digest rejects the caller but settles only when the digest finishes', async () => {
    const started = Promise.withResolvers<void>()
    const digest = Promise.withResolvers<ArrayBuffer>()
    let digestCalls = 0
    const assembly = new FileAssembly(binary.length, () => {
      digestCalls++
      started.resolve()
      return digest.promise
    })
    assembly.append(binary)
    const result = assembly.complete(binaryHash)
    let settled = false
    await started.promise
    void assembly.settled.then(() => {
      settled = true
    })

    assembly.cancel()
    await assert.rejects(result, /File read cancelled/)
    assert.equal(settled, false)

    digest.resolve(new Uint8Array(32).buffer)
    await assembly.settled
    assert.equal(settled, true)
    assert.equal(digestCalls, 1)
  })
})
