import type { TextRevealTarget } from '$lib/features/workbench/surfaces/text/service.svelte'

export interface ParsedLinkTarget {
  raw: string
  kind: 'web' | 'uri' | 'file' | 'anchor'
  url?: string
  scheme?: string
  path?: string
  reveal?: TextRevealTarget | null
}

const SCHEME_REGEX = /^([a-zA-Z][a-zA-Z0-9+.-]*):\/\/(.*)$/

/**
 * Strip surrounding quotes, backticks, parens, brackets from raw terminal or markdown token.
 */
export function sanitizeLinkCandidate(raw: string): string {
  let text = raw.trim()
  if (
    (text.startsWith('"') && text.endsWith('"')) ||
    (text.startsWith("'") && text.endsWith("'")) ||
    (text.startsWith('`') && text.endsWith('`'))
  ) {
    text = text.slice(1, -1).trim()
  }
  if (text.startsWith('(') && text.endsWith(')')) {
    text = text.slice(1, -1).trim()
  }
  return text
}

/**
 * Parse path and line/col indicators from a path string.
 * Supports:
 * - `path/to/file.ext:42:15-80`
 * - `path/to/file.ext:42:15`
 * - `path/to/file.ext:42`
 * - `path/to/file.ext:42-100`
 * - `path/to/file.ext#L42-L50`
 * - `path/to/file.ext#L42`
 * - `[path/to/file.ext#TAG]`
 */
export function parseFilePathAndLocation(input: string): {
  filePath: string
  reveal: TextRevealTarget | null
} {
  const text = sanitizeLinkCandidate(input)

  // Handle [path/to/file#TAG] hashline snapshot tags
  const hashlineMatch = text.match(/^\[(.+?)#[0-9a-fA-F]+\]$/)
  if (hashlineMatch) {
    return { filePath: hashlineMatch[1], reveal: null }
  }

  // Suffix matching: #L<n>[-L<m>], :<line>:<col>[-<endCol>], :<line>[-<endLine>], :<line>:<col>, :<line>, or non-numeric #fragment
  const suffixMatch = text.match(
    /^(.*?)(?:#(?:L?(\d+)(?:-L?(\d+))?|[a-zA-Z0-9_.-]+)|:(\d+)(?::(\d+)(?:-(\d+))?|-(\d+))?)$/
  )
  if (!suffixMatch) {
    return { filePath: text, reveal: null }
  }

  const [, filePath, hashL1, hashL2, colonL1, colonC1, colonC2, colonL2] = suffixMatch
  if (colonL1) {
    const l1 = parseInt(colonL1, 10)
    if (colonC1) {
      const c1 = parseInt(colonC1, 10)
      if (colonC2) {
        return {
          filePath,
          reveal: {
            kind: 'range',
            startLineNumber: l1,
            startColumn: c1,
            endLineNumber: l1,
            endColumn: parseInt(colonC2, 10)
          }
        }
      }
      return {
        filePath,
        reveal: {
          kind: 'position',
          lineNumber: l1,
          column: c1
        }
      }
    }
    if (colonL2) {
      return {
        filePath,
        reveal: {
          kind: 'range',
          startLineNumber: l1,
          startColumn: 1,
          endLineNumber: parseInt(colonL2, 10),
          endColumn: 1
        }
      }
    }
    return {
      filePath,
      reveal: {
        kind: 'position',
        lineNumber: l1,
        column: 1
      }
    }
  }

  if (hashL1) {
    const l1 = parseInt(hashL1, 10)
    if (hashL2) {
      return {
        filePath,
        reveal: {
          kind: 'range',
          startLineNumber: l1,
          startColumn: 1,
          endLineNumber: parseInt(hashL2, 10),
          endColumn: 1
        }
      }
    }
    return {
      filePath,
      reveal: {
        kind: 'position',
        lineNumber: l1,
        column: 1
      }
    }
  }

  return { filePath, reveal: null }
}

/**
 * Classify a raw link token into web URL, URI scheme, file path, or anchor fragment.
 */
export function parseLinkTarget(raw: string): ParsedLinkTarget {
  const sanitized = sanitizeLinkCandidate(raw)

  // In-page anchor fragments
  if (sanitized.startsWith('#')) {
    return {
      raw,
      kind: 'anchor',
      path: sanitized
    }
  }

  // Web & communication schemes
  if (
    sanitized.startsWith('http://') ||
    sanitized.startsWith('https://') ||
    sanitized.startsWith('mailto:') ||
    sanitized.startsWith('tel:')
  ) {
    return {
      raw,
      kind: 'web',
      url: sanitized
    }
  }

  const schemeMatch = sanitized.match(SCHEME_REGEX)
  if (schemeMatch) {
    const scheme = schemeMatch[1].toLowerCase()
    return {
      raw,
      kind: 'uri',
      scheme,
      url: sanitized
    }
  }

  const { filePath, reveal } = parseFilePathAndLocation(sanitized)
  return {
    raw,
    kind: 'file',
    path: filePath,
    reveal
  }
}
