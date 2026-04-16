// Extension-to-Monaco-language mapping and binary file detection.
//
// Monaco language IDs sometimes differ from Shiki's (e.g. "shellscript"
// vs "bash", "plaintext" vs "text"). This map is tailored for Monaco's
// built-in Monarch tokenizers.

// Map of file extension (without dot) to Monaco language ID.
const EXT_TO_LANG: Record<string, string> = {
  // -- Web fundamentals --
  js: 'javascript',
  mjs: 'javascript',
  cjs: 'javascript',
  jsx: 'javascript',
  ts: 'typescript',
  mts: 'typescript',
  cts: 'typescript',
  tsx: 'typescript',
  html: 'html',
  htm: 'html',
  css: 'css',
  scss: 'scss',
  less: 'less',
  json: 'json',
  jsonc: 'json',
  json5: 'json',

  // -- Markup / docs --
  md: 'markdown',
  mdx: 'markdown',
  xml: 'xml',
  svg: 'xml',
  xsl: 'xml',
  xslt: 'xml',
  yaml: 'yaml',
  yml: 'yaml',
  toml: 'ini', // closest built-in match
  ini: 'ini',
  conf: 'ini',

  // -- Systems --
  rs: 'rust',
  go: 'go',
  c: 'c',
  h: 'c',
  cpp: 'cpp',
  cxx: 'cpp',
  cc: 'cpp',
  hpp: 'cpp',
  hxx: 'cpp',
  cs: 'csharp',
  java: 'java',
  kt: 'kotlin',
  kts: 'kotlin',
  scala: 'scala',
  swift: 'swift',
  m: 'objective-c',
  mm: 'objective-c',
  zig: 'zig',

  // -- Scripting --
  py: 'python',
  pyw: 'python',
  pyi: 'python',
  rb: 'ruby',
  rake: 'ruby',
  gemspec: 'ruby',
  pl: 'perl',
  pm: 'perl',
  lua: 'lua',
  php: 'php',
  r: 'r',
  R: 'r',
  dart: 'dart',
  ex: 'elixir',
  exs: 'elixir',
  erl: 'erlang',
  hrl: 'erlang',
  clj: 'clojure',
  cljs: 'clojure',
  cljc: 'clojure',
  fs: 'fsharp',
  fsi: 'fsharp',
  fsx: 'fsharp',
  hs: 'haskell',
  lhs: 'haskell',
  jl: 'julia',
  tcl: 'tcl',
  groovy: 'groovy',
  gradle: 'groovy',
  ps1: 'powershell',
  psm1: 'powershell',

  // -- Shell --
  sh: 'shell',
  bash: 'shell',
  zsh: 'shell',
  fish: 'fish', // Shiki TextMate grammar via @shikijs/monaco
  ksh: 'shell',

  // -- Data / query --
  sql: 'sql',
  graphql: 'graphql',
  gql: 'graphql',
  proto: 'protobuf',

  // -- Config / build --
  dockerfile: 'dockerfile',
  tf: 'hcl',
  hcl: 'hcl',
  cmake: 'cmake',

  // -- Svelte / Vue / frameworks --
  svelte: 'svelte', // Shiki TextMate grammar via @shikijs/monaco
  vue: 'html',

  // -- Misc --
  bat: 'bat',
  cmd: 'bat',
  coffee: 'coffeescript',
  tex: 'latex',
  latex: 'latex',
  diff: 'diff',
  patch: 'diff',
  log: 'log',
  txt: 'plaintext',
  lock: 'plaintext',
  nix: 'nix' // Shiki TextMate grammar via @shikijs/monaco
}

// Exact filenames that map to a language (no extension needed).
const FILENAME_TO_LANG: Record<string, string> = {
  Dockerfile: 'dockerfile',
  Makefile: 'makefile',
  Rakefile: 'ruby',
  Gemfile: 'ruby',
  CMakeLists: 'cmake',
  '.gitignore': 'plaintext',
  '.gitattributes': 'plaintext',
  '.editorconfig': 'ini',
  '.env': 'ini',
  '.env.local': 'ini',
  '.env.example': 'ini',
  'tsconfig.json': 'json',
  'package.json': 'json',
  'flake.nix': 'nix',
  'flake.lock': 'json'
}

// Binary extensions that should not be opened in the editor.
// SVG is intentionally excluded — it's XML text, not binary.
const BINARY_EXTENSIONS = new Set([
  'png',
  'jpg',
  'jpeg',
  'gif',
  'bmp',
  'ico',
  'webp',
  'avif',
  'mp3',
  'mp4',
  'wav',
  'ogg',
  'webm',
  'flac',
  'avi',
  'mov',
  'mkv',
  'pdf',
  'zip',
  'tar',
  'gz',
  'bz2',
  'xz',
  'zst',
  '7z',
  'rar',
  'wasm',
  'exe',
  'dll',
  'so',
  'dylib',
  'o',
  'a',
  'lib',
  'bin',
  'dat',
  'db',
  'sqlite',
  'sqlite3',
  'ttf',
  'otf',
  'woff',
  'woff2',
  'ttc',
  'eot'
])

function getExtension(filePath: string): string {
  const name = filePath.split('/').pop() ?? filePath
  if (!name.includes('.')) return ''
  return (name.split('.').pop() ?? '').toLowerCase()
}

/** Resolve a file path to a Monaco language ID. */
export function filePathToLanguage(filePath: string): string {
  const fileName = filePath.split('/').pop() ?? filePath
  const baseName = fileName.split('.').slice(0, -1).join('.') || fileName

  if (FILENAME_TO_LANG[fileName]) return FILENAME_TO_LANG[fileName]
  if (FILENAME_TO_LANG[baseName]) return FILENAME_TO_LANG[baseName]

  return EXT_TO_LANG[getExtension(filePath)] ?? 'plaintext'
}

export function isBinaryFile(filePath: string): boolean {
  return BINARY_EXTENSIONS.has(getExtension(filePath))
}

export function isMarkdownFile(filePath: string): boolean {
  const ext = getExtension(filePath)
  return ext === 'md' || ext === 'mdx'
}
