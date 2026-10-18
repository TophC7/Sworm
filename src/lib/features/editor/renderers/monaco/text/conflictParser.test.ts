import test, { describe } from 'node:test'
import assert from 'node:assert/strict'
import {
  getConflictEditRange,
  parseConflictBlocks,
  resolveConflictBlock,
  type ConflictResolution
} from './conflictParser'

const twoWay = '<<<<<<< HEAD\nour first\nour second\n=======\ntheir first\ntheir second\n>>>>>>> feature'
const threeWay =
  '<<<<<<< current branch\nours\n||||||| common ancestor\nold first\nold second\n=======\ntheirs\n>>>>>>> incoming branch'

/** Apply the parser's actual edit bounds as character offsets, including EOF clamping. */
function resolveText(source: string, resolution: ConflictResolution, all = false): string {
  const lines = source.split('\n')
  const blocks = parseConflictBlocks(lines)
  const selected = all ? blocks : blocks.slice(0, 1)
  let result = source
  for (const block of selected.toReversed()) {
    const range = getConflictEditRange(block, lines.length)
    const offsetAt = (line: number, column: number) =>
      lines.slice(0, line - 1).reduce((offset, text) => offset + text.length + 1, 0) +
      Math.min(column - 1, lines[line - 1].replace(/\r$/, '').length)
    result =
      result.slice(0, offsetAt(range.startLineNumber, range.startColumn)) +
      resolveConflictBlock(lines, block, resolution) +
      result.slice(offsetAt(range.endLineNumber, range.endColumn))
  }
  return result
}

describe('parseConflictBlocks', () => {
  test('parses two-way multiline markers and branch labels', () => {
    assert.deepEqual(parseConflictBlocks(twoWay.split('\n')), [
      {
        startLine: 1,
        startHeader: 'HEAD',
        commonLine: null,
        commonHeader: null,
        splitterLine: 4,
        endLine: 7,
        endHeader: 'feature'
      }
    ])
  })

  test('parses a common ancestor without treating it as current content', () => {
    assert.deepEqual(parseConflictBlocks(threeWay.split('\n')), [
      {
        startLine: 1,
        startHeader: 'current branch',
        commonLine: 3,
        commonHeader: 'common ancestor',
        splitterLine: 6,
        endLine: 8,
        endHeader: 'incoming branch'
      }
    ])
  })

  test('parses consecutive blocks, including empty sides and unlabeled headers', () => {
    const source = '<<<<<<<\n=======\n>>>>>>>\n<<<<<<< ours\na\n=======\nb\n>>>>>>> theirs'
    assert.deepEqual(parseConflictBlocks(source.split('\n')), [
      {
        startLine: 1,
        startHeader: '',
        commonLine: null,
        commonHeader: null,
        splitterLine: 2,
        endLine: 3,
        endHeader: ''
      },
      {
        startLine: 4,
        startHeader: 'ours',
        commonLine: null,
        commonHeader: null,
        splitterLine: 6,
        endLine: 8,
        endHeader: 'theirs'
      }
    ])
  })

  test('accepts CRLF lines and tab-separated labels', () => {
    const blocks = parseConflictBlocks(
      '<<<<<<<\tHEAD\r\na\r\n|||||||\tbase\r\no\r\n=======\r\nb\r\n>>>>>>>\ttopic\r\n'.split('\n')
    )
    assert.deepEqual(blocks, [
      {
        startLine: 1,
        startHeader: 'HEAD',
        commonLine: 3,
        commonHeader: 'base',
        splitterLine: 5,
        endLine: 7,
        endHeader: 'topic'
      }
    ])
  })

  const malformedInputs: string[] = [
    '',
    'ordinary text\n=======\n>>>>>>> end',
    '<<<<<<< start\nno separator\n>>>>>>> end',
    '<<<<<<< start\nunfinished',
    '<<<<<<< start\na\n=======\nb',
    '<<<<<<< start\n||||||| base\n||||||| duplicate\n=======\n>>>>>>> end',
    '<<<<<<< start\n=======\n=======\n>>>>>>> end',
    '<<<<<<< start\n=======\n||||||| late base\n>>>>>>> end',
    ' <<<<<<< indented\na\n=======\nb\n>>>>>>> end',
    '<<<<<<<< longer\na\n=======\nb\n>>>>>>>> longer',
    '<<<<<<< start\na\n======= not a separator\nb\n>>>>>>> end'
  ]
  for (const source of malformedInputs) {
    test(`ignores malformed or incomplete input: ${JSON.stringify(source)}`, () => {
      assert.deepEqual(parseConflictBlocks(source.split('\n')), [])
    })
  }

  const malformedPrefixes: string[] = [
    '<<<<<<< unfinished\ntext',
    '<<<<<<< invalid\n=======\n=======\n>>>>>>> end',
    '>>>>>>> orphan\n||||||| orphan\n======='
  ]
  for (const prefix of malformedPrefixes) {
    test(`recovers at a new start after malformed input: ${JSON.stringify(prefix)}`, () => {
      const lines = `${prefix}\n${twoWay}`.split('\n')
      const blocks = parseConflictBlocks(lines)
      assert.equal(blocks.length, 1)
      assert.equal(blocks[0].startLine, prefix.split('\n').length + 1)
      assert.equal(blocks[0].endHeader, 'feature')
    })
  }

  test('does not discard a completed conflict when a later block is incomplete', () => {
    assert.equal(parseConflictBlocks(`${twoWay}\n<<<<<<< unfinished\na`.split('\n')).length, 1)
  })
})

describe('resolveConflictBlock and edit ranges', () => {
  const multilineResolutions: [ConflictResolution, string][] = [
    ['current', 'our first\nour second\n'],
    ['incoming', 'their first\ntheir second\n'],
    ['both', 'our first\nour second\ntheir first\ntheir second\n']
  ]
  for (const [resolution, expected] of multilineResolutions) {
    test(`resolves multiline ${resolution} at EOF`, () => {
      assert.equal(resolveText(twoWay, resolution), expected)
    })
  }

  const ancestorResolutions: [ConflictResolution, string][] = [
    ['current', 'ours\n'],
    ['incoming', 'theirs\n'],
    ['both', 'ours\ntheirs\n']
  ]
  for (const [resolution, expected] of ancestorResolutions) {
    test(`excludes the common ancestor when accepting ${resolution}`, () => {
      assert.equal(resolveText(threeWay, resolution), expected)
    })
  }

  const resolutions: ConflictResolution[] = ['current', 'incoming', 'both']
  for (const resolution of resolutions) {
    test(`preserves surrounding blank lines for ${resolution}`, () => {
      const expected = resolveText(twoWay, resolution)
      assert.equal(resolveText(`before\n\n${twoWay}\n\nafter\n`, resolution), `before\n\n${expected}\nafter\n`)
      assert.equal(resolveText(`${twoWay}\nafter`, resolution), `${expected}after`)
      assert.equal(resolveText(`before\n${twoWay}`, resolution), `before\n${expected}`)
      assert.equal(resolveText(`${twoWay}\n`, resolution), expected)
    })
  }

  const emptySides: [string, ConflictResolution][] = [
    ['<<<<<<< HEAD\n=======\ntheirs\n>>>>>>> topic', 'current'],
    ['<<<<<<< HEAD\nours\n=======\n>>>>>>> topic', 'incoming'],
    ['<<<<<<< HEAD\n=======\n>>>>>>> topic', 'both']
  ]
  for (const [source, resolution] of emptySides) {
    test(`removes an empty selected ${resolution} side without inventing a blank line`, () => {
      assert.equal(resolveText(source, resolution), '')
      assert.equal(resolveText(`${source}\n`, resolution), '')
      assert.equal(resolveText(`before\n${source}\nafter\n`, resolution), 'before\nafter\n')
      assert.equal(resolveText(`before\n${source}`, resolution), 'before\n')
      assert.equal(resolveText(source.replaceAll('\n', '\r\n'), resolution), '')
      assert.equal(
        resolveText(`before\n${source}\nafter\n`.replaceAll('\n', '\r\n'), resolution),
        'before\r\nafter\r\n'
      )
    })
  }

  const oneEmptySide: [string, string][] = [
    ['<<<<<<< HEAD\n=======\ntheirs\n>>>>>>> topic', 'theirs\n'],
    ['<<<<<<< HEAD\nours\n=======\n>>>>>>> topic', 'ours\n']
  ]
  for (const [source, expected] of oneEmptySide) {
    test(`accepts both with one empty side without adding blank content: ${JSON.stringify(source)}`, () => {
      assert.equal(resolveText(source, 'both'), expected)
      assert.equal(resolveText(`${source}\nafter`, 'both'), `${expected}after`)
    })
  }

  test('retains intentional blank content lines rather than treating them as an empty side', () => {
    const source = '<<<<<<< HEAD\n\n=======\n\n>>>>>>> topic'
    assert.equal(resolveText(`${source}\nafter`, 'current'), '\nafter')
    assert.equal(resolveText(`${source}\nafter`, 'both'), '\n\nafter')
    assert.equal(resolveText(source, 'current'), '\n')
    assert.equal(resolveText(source, 'incoming'), '\n')
    assert.equal(resolveText(source, 'both'), '\n\n')
  })

  for (const resolution of resolutions) {
    test(`preserves CRLF content terminators and surrounding text for ${resolution}`, () => {
      const source = `before\n\n${threeWay}\n\nafter\n`.replaceAll('\n', '\r\n')
      const expected = resolution === 'current' ? 'ours' : resolution === 'incoming' ? 'theirs' : 'ours\r\ntheirs'
      assert.equal(resolveText(source, resolution), `before\r\n\r\n${expected}\r\n\r\nafter\r\n`)
      assert.equal(resolveText(threeWay.replaceAll('\n', '\r\n'), resolution), `${expected}\r\n`)
      assert.equal(resolveText(`${threeWay}\n`.replaceAll('\n', '\r\n'), resolution), `${expected}\r\n`)
    })
  }

  test('accepts an explicit model EOL when model lines have no terminators', () => {
    const lines = `${twoWay}\n`.split('\n')
    assert.equal(
      resolveConflictBlock(lines, parseConflictBlocks(lines)[0], 'current', '\r\n'),
      'our first\r\nour second\r\n'
    )
  })

  for (const resolution of resolutions) {
    test(`resolves all consecutive blocks against one snapshot for ${resolution}`, () => {
      const source = `${twoWay}\n${threeWay}\ntail\n`
      assert.equal(
        resolveText(source, resolution, true),
        `${resolveText(twoWay, resolution)}${resolveText(threeWay, resolution)}tail\n`
      )
      assert.deepEqual(parseConflictBlocks(resolveText(source, resolution, true).split('\n')), [])
    })
  }

  test('calculates beginning and middle ranges through the marker newline', () => {
    const startLines = `${twoWay}\nafter`.split('\n')
    assert.deepEqual(getConflictEditRange(parseConflictBlocks(startLines)[0], startLines.length), {
      startLineNumber: 1,
      startColumn: 1,
      endLineNumber: 8,
      endColumn: 1
    })
    const middleLines = `before\n${twoWay}\nafter`.split('\n')
    assert.deepEqual(getConflictEditRange(parseConflictBlocks(middleLines)[0], middleLines.length), {
      startLineNumber: 2,
      startColumn: 1,
      endLineNumber: 9,
      endColumn: 1
    })
  })

  test('uses an EOF max-column sentinel only when no line follows the end marker', () => {
    const lines = twoWay.split('\n')
    assert.deepEqual(getConflictEditRange(parseConflictBlocks(lines)[0], lines.length), {
      startLineNumber: 1,
      startColumn: 1,
      endLineNumber: 7,
      endColumn: Number.MAX_SAFE_INTEGER
    })
    const withNewline = `${twoWay}\n`.split('\n')
    assert.deepEqual(getConflictEditRange(parseConflictBlocks(withNewline)[0], withNewline.length), {
      startLineNumber: 1,
      startColumn: 1,
      endLineNumber: 8,
      endColumn: 1
    })
  })
})
