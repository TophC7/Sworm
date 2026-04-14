/**
 * Utilities for parsing raw `git diff` output metadata and
 * detecting binary diffs.
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

/**
 * Maps file extensions to Shiki language identifiers.
 * This is the single source of truth for extension → language mapping.
 */
const EXT_LANG_MAP: Record<string, string> = {
  ts: 'typescript',
  tsx: 'tsx',
  js: 'javascript',
  jsx: 'jsx',
  mjs: 'javascript',
  cjs: 'javascript',
  svelte: 'svelte',
  vue: 'vue',
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
  html: 'html',
  htm: 'html',
  xml: 'xml',
  svg: 'xml',
  json: 'json',
  yaml: 'yaml',
  yml: 'yaml',
  toml: 'toml',
  ini: 'ini',
  md: 'markdown',
  mdx: 'mdx',
  sh: 'bash',
  bash: 'bash',
  fish: 'fish',
  zsh: 'shellscript',
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
 * Map a file path's extension to a Shiki language identifier.
 * Falls back to 'plaintext' for unknown extensions.
 */
export function extToHighlightLang(filePath: string): string {
  const ext = filePath.split('.').pop()?.toLowerCase() ?? ''
  return EXT_LANG_MAP[ext] ?? 'plaintext'
}
