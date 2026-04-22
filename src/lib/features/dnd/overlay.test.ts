import test from 'node:test'
import assert from 'node:assert/strict'
import { computeZone } from '$lib/features/dnd/overlay'

const WIDTH = 100
const HEIGHT = 100

test('returns merge when split is disabled', () => {
  assert.equal(computeZone(10, 10, WIDTH, HEIGHT, { allowSplit: false }), 'merge')
})

test('returns merge at center', () => {
  assert.equal(computeZone(50, 50, WIDTH, HEIGHT, { allowSplit: true }), 'merge')
})

test('returns left near left edge', () => {
  assert.equal(computeZone(2, 50, WIDTH, HEIGHT, { allowSplit: true }), 'left')
})

test('returns right near right edge', () => {
  assert.equal(computeZone(98, 50, WIDTH, HEIGHT, { allowSplit: true }), 'right')
})

test('returns up near top edge', () => {
  assert.equal(computeZone(50, 2, WIDTH, HEIGHT, { allowSplit: true }), 'up')
})

test('returns down near bottom edge', () => {
  assert.equal(computeZone(50, 98, WIDTH, HEIGHT, { allowSplit: true }), 'down')
})

test('returns left at top-left corner', () => {
  assert.equal(computeZone(1, 1, WIDTH, HEIGHT, { allowSplit: true }), 'left')
})

test('returns right at top-right corner', () => {
  assert.equal(computeZone(99, 1, WIDTH, HEIGHT, { allowSplit: true }), 'right')
})

test('returns left at bottom-left corner', () => {
  assert.equal(computeZone(1, 99, WIDTH, HEIGHT, { allowSplit: true }), 'left')
})

test('returns right at bottom-right corner', () => {
  assert.equal(computeZone(99, 99, WIDTH, HEIGHT, { allowSplit: true }), 'right')
})
