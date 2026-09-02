const ESC = '\x1b'
const BEL = '\x07'
const CAN = '\x18'
const SUB = '\x1a'
const MAX_TITLE_CODE_POINTS = 4096
const MAX_DISPLAY_CODE_POINTS = 240

const INVISIBLE_CONTROLS =
  /[\u00ad\u034f\u061c\u180e\u200b-\u200c\u200e-\u200f\u202a-\u202e\u2060-\u206f\ufe00-\ufe0f\ufeff\ufff9-\ufffb\u{1bca0}-\u{1bca3}\u{e0100}-\u{e01ef}]/gu
const EXTENDED_PICTOGRAPHIC = /^\p{Extended_Pictographic}$/u
const EMOJI_MODIFIER = /^\p{Emoji_Modifier}$/u

type ParserState = 'text' | 'escape' | 'command' | 'title' | 'titleEscape' | 'discard' | 'discardEscape'

function normalizeTitle(title: string): string | null {
  const codePoints = [
    ...title
      .replace(/\p{White_Space}+/gu, ' ')
      .replace(/[\u0000-\u001f\u007f-\u009f]/g, '')
      .replace(INVISIBLE_CONTROLS, '')
  ]

  const normalized = codePoints
    .filter((char, index) => {
      if (char !== '\u200d') return true
      let previous = index - 1
      while (previous >= 0 && EMOJI_MODIFIER.test(codePoints[previous])) previous -= 1
      return (
        EXTENDED_PICTOGRAPHIC.test(codePoints[previous] ?? '') &&
        EXTENDED_PICTOGRAPHIC.test(codePoints[index + 1] ?? '')
      )
    })
    .join('')
    .trim()

  if (!normalized) return null
  return [...normalized].slice(0, MAX_DISPLAY_CODE_POINTS).join('')
}

export class TerminalTitleParser {
  private state: ParserState = 'text'
  private command = ''
  private title = ''
  private titleLength = 0

  reset(): void {
    this.state = 'text'
    this.command = ''
    this.title = ''
    this.titleLength = 0
  }

  push(chunk: string): string | null {
    if (this.state === 'text' && !chunk.includes(ESC)) return null

    let latest: string | null = null

    for (const char of chunk) {
      switch (this.state) {
        case 'text':
          if (char === ESC) this.state = 'escape'
          break
        case 'escape':
          if (char === ']') {
            this.state = 'command'
            this.command = ''
          } else {
            this.state = char === ESC ? 'escape' : 'text'
          }
          break
        case 'command':
          if (char === BEL || char === CAN || char === SUB) {
            this.finishSequence()
          } else if (char === ESC) {
            this.state = 'discardEscape'
          } else if (this.command === '' && (char === '0' || char === '2')) {
            this.command = char
          } else if (this.command !== '' && char === ';') {
            this.state = 'title'
          } else {
            this.state = 'discard'
          }
          break
        case 'title':
          if (char === BEL) {
            latest = this.completeTitle() ?? latest
          } else if (char === CAN || char === SUB) {
            this.finishSequence()
          } else if (char === ESC) {
            this.state = 'titleEscape'
          } else {
            this.appendTitle(char)
          }
          break
        case 'titleEscape':
          if (char === '\\' || char === BEL) {
            latest = this.completeTitle() ?? latest
          } else if (char === CAN || char === SUB) {
            this.finishSequence()
          } else if (char === ESC) {
            this.appendTitle(ESC)
          } else {
            this.appendTitle(ESC)
            if (this.state === 'titleEscape') {
              this.state = 'title'
              this.appendTitle(char)
            }
          }
          break
        case 'discard':
          if (char === BEL || char === CAN || char === SUB) {
            this.finishSequence()
          } else if (char === ESC) {
            this.state = 'discardEscape'
          }
          break
        case 'discardEscape':
          if (char === '\\' || char === BEL || char === CAN || char === SUB) {
            this.finishSequence()
          } else if (char !== ESC) {
            this.state = 'discard'
          }
          break
      }
    }

    return latest
  }

  private appendTitle(char: string): void {
    this.titleLength += 1
    if (this.titleLength > MAX_TITLE_CODE_POINTS) {
      this.title = ''
      this.state = 'discard'
      return
    }
    this.title += char
  }

  private completeTitle(): string | null {
    const title = normalizeTitle(this.title)
    this.finishSequence()
    return title
  }

  private finishSequence(): void {
    this.state = 'text'
    this.command = ''
    this.title = ''
    this.titleLength = 0
  }
}
