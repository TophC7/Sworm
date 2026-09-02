import test, { describe } from 'node:test'
import assert from 'node:assert/strict'
import { TerminalTitleParser } from './terminalTitle'

const ESC = '\x1b'
const BEL = '\x07'

function parse(input: string): string | null {
  return new TerminalTitleParser().push(input)
}

function codePoints(start: number, end = start): string {
  return Array.from({ length: end - start + 1 }, (_, offset) => String.fromCodePoint(start + offset)).join('')
}

describe('TerminalTitleParser', () => {
  test('parses OSC 0 with BEL and OSC 2 with ST', () => {
    assert.equal(parse(`${ESC}]0;Shell title${BEL}`), 'Shell title')
    assert.equal(parse(`${ESC}]2;Program title${ESC}\\`), 'Program title')
  })

  test('preserves state across every sequence split point', () => {
    const sequence = `${ESC}]2;Split title${ESC}\\`

    for (let split = 0; split <= sequence.length; split += 1) {
      const parser = new TerminalTitleParser()
      let title = parser.push(sequence.slice(0, split))
      title = parser.push(sequence.slice(split)) ?? title
      assert.equal(title, 'Split title', `split at ${split}`)
    }
  })

  test('preserves visible emoji and zero-width joiners', () => {
    assert.equal(parse(`${ESC}]0;Building 👩‍💻 workspace${BEL}`), 'Building 👩‍💻 workspace')
  })

  test('returns the last usable title in one chunk', () => {
    assert.equal(parse(`${ESC}]0;First${BEL}${ESC}]2;Second${BEL}`), 'Second')
    assert.equal(parse(`${ESC}]0;First${BEL}${ESC}]2;\x00\x01${BEL}`), 'First')
  })

  test('ignores unsupported and clipboard OSC commands', () => {
    const parser = new TerminalTitleParser()
    assert.equal(parser.push(`${ESC}]20;icon${BEL}${ESC}]52;c;clipboard${ESC}\\`), null)
    assert.equal(parser.push(`${ESC}]0;Usable${BEL}`), 'Usable')
  })

  test('rejects empty and control-only titles', () => {
    assert.equal(parse(`${ESC}]0;${BEL}`), null)
    assert.equal(parse(`${ESC}]0;\x00\x01\x7f\x80\x9f${BEL}`), null)
  })

  test('normalizes whitespace before stripping other controls', () => {
    assert.equal(parse(`${ESC}]0;  Alpha\t\r\n Beta\x01Gamma  ${BEL}`), 'Alpha BetaGamma')
  })

  test('strips every configured invisible control range', () => {
    const invisible = [
      codePoints(0x00ad),
      codePoints(0x034f),
      codePoints(0x061c),
      codePoints(0x180e),
      codePoints(0x200b, 0x200f),
      codePoints(0x202a, 0x202e),
      codePoints(0x2060, 0x206f),
      codePoints(0xfe00, 0xfe0f),
      codePoints(0xfeff),
      codePoints(0xfff9, 0xfffb),
      codePoints(0x1bca0, 0x1bca3),
      codePoints(0xe0100, 0xe01ef)
    ].join('')

    assert.equal(parse(`${ESC}]0;Before${invisible}After${BEL}`), 'BeforeAfter')
  })

  test('limits display titles to 240 Unicode code points', () => {
    const title = `${'x'.repeat(239)}👩z`
    assert.equal(parse(`${ESC}]0;${title}${BEL}`), `${'x'.repeat(239)}👩`)
  })

  test('aborts candidates on CAN or SUB and recovers', () => {
    for (const abort of ['\x18', '\x1a']) {
      assert.equal(parse(`${ESC}]0;Discard${abort}${ESC}]2;Recovered${BEL}`), 'Recovered')
    }
  })

  test('discards overflow through its terminator before parsing another title', () => {
    const overflowWithNestedOsc = `${'x'.repeat(4097)}${ESC}]0;Nested${BEL}`
    assert.equal(parse(`${ESC}]0;${overflowWithNestedOsc}${ESC}]2;Recovered${BEL}`), 'Recovered')
  })

  test('reset prevents an old partial sequence joining a new run', () => {
    const parser = new TerminalTitleParser()
    assert.equal(parser.push(`${ESC}]0;Old`), null)
    parser.reset()
    assert.equal(parser.push(` continuation${BEL}`), null)
    assert.equal(parser.push(`${ESC}]0;Fresh${BEL}`), 'Fresh')
  })
})
