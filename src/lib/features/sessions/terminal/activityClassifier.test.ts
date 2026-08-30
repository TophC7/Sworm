import test, { describe } from 'node:test'
import assert from 'node:assert/strict'
import { classifyActivity } from './activityClassifier'

describe('classifyActivity for OMP', () => {
  test('detects busy on thinking or working', () => {
    assert.equal(classifyActivity('omp', 'Working...'), 'busy')
    assert.equal(classifyActivity('omp', 'Thinking about the architecture'), 'busy')
    assert.equal(classifyActivity('omp', 'Press Esc to interrupt'), 'busy')
  })

  test('detects idle when awaiting input', () => {
    assert.equal(classifyActivity('omp', 'Ask a question or describe a task'), 'idle')
    assert.equal(classifyActivity('omp', 'Send a message (Enter to send)'), 'idle')
  })
})
