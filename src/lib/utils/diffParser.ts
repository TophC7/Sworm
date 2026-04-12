/**
 * Utilities for parsing raw `git diff` output into the format
 * expected by @git-diff-view/svelte's data prop.
 */

/** File metadata extracted from diff header lines. */
export interface DiffMeta {
  oldFileName: string | undefined
  newFileName: string | undefined
}

/**
 * Extract old/new file names from diff header lines.
 * Handles `/dev/null` for new/deleted files.
 *
 * Only scans up to the first hunk header to avoid splitting
 * the entire diff into lines for large files.
 */
export function parseDiffMeta(rawDiff: string): DiffMeta {
  let oldFileName: string | undefined
  let newFileName: string | undefined

  // Limit the search to the header region before the first hunk
  const hunkStart = rawDiff.indexOf('\n@@')
  const header = hunkStart >= 0 ? rawDiff.slice(0, hunkStart) : rawDiff

  for (const line of header.split('\n')) {
    if (line.startsWith('--- ')) {
      const path = line.slice(4)
      oldFileName = path === '/dev/null' ? undefined : path.replace(/^[ab]\//, '')
    } else if (line.startsWith('+++ ')) {
      const path = line.slice(4)
      newFileName = path === '/dev/null' ? undefined : path.replace(/^[ab]\//, '')
    }
  }

  return { oldFileName, newFileName }
}

/** Check whether a raw diff describes a binary file. */
export function isBinaryDiff(rawDiff: string): boolean {
  return rawDiff.includes('Binary files') && rawDiff.includes('differ')
}

const EXT_LANG_MAP: Record<string, string> = {
  ts: 'typescript',
  tsx: 'typescript',
  js: 'javascript',
  jsx: 'javascript',
  mjs: 'javascript',
  cjs: 'javascript',
  svelte: 'xml',
  vue: 'xml',
  rs: 'rust',
  py: 'python',
  rb: 'ruby',
  go: 'go',
  java: 'java',
  kt: 'kotlin',
  c: 'c',
  h: 'c',
  cpp: 'cpp',
  hpp: 'cpp',
  cs: 'csharp',
  swift: 'swift',
  css: 'css',
  scss: 'scss',
  less: 'less',
  html: 'xml',
  htm: 'xml',
  xml: 'xml',
  svg: 'xml',
  json: 'json',
  yaml: 'yaml',
  yml: 'yaml',
  toml: 'ini',
  ini: 'ini',
  md: 'markdown',
  mdx: 'markdown',
  sh: 'bash',
  bash: 'bash',
  fish: 'bash',
  zsh: 'bash',
  sql: 'sql',
  graphql: 'graphql',
  gql: 'graphql',
  dockerfile: 'dockerfile',
  nix: 'nix',
  lua: 'lua',
  zig: 'zig',
  diff: 'diff',
  lock: 'plaintext'
}

/**
 * Map a file path's extension to a highlight.js language identifier.
 * Falls back to 'plaintext' for unknown extensions.
 */
export function extToHighlightLang(filePath: string): string {
  const ext = filePath.split('.').pop()?.toLowerCase() ?? ''
  return EXT_LANG_MAP[ext] ?? 'plaintext'
}
