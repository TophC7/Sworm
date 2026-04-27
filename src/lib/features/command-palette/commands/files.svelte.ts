import type { Command, CommandGroup } from './types'
import { getProjectFilePaths } from '$lib/features/files/projectFiles.svelte'
import { openTextFile } from '$lib/features/workbench/surfaces/text/service.svelte'
import { basename, dirname } from '$lib/utils/paths'

// Cap on how many file results to feed bits-ui at once. The palette
// renders every row, so on monorepos with 25k files an unfiltered list
// would blow the frame budget. 200 keeps the list responsive while
// still letting fuzzy queries surface plenty of matches.
const MAX_RESULTS = 200

export interface FileQuery {
  /** The path query with any trailing `:line` stripped. */
  path: string
  /** Optional 1-based line target parsed from a `:line` suffix. */
  line: number | null
}

/**
 * Parse a file-mode query of shape `path` or `path:line`. The line
 * suffix is only honored when the trailing token is purely digits;
 * paths happen to contain colons on Windows but Sworm is Linux-first
 * and POSIX paths never include `:`.
 */
export function parseFileQuery(raw: string): FileQuery {
  const trimmed = raw.trim()
  const match = trimmed.match(/^(.*):(\d+)$/)
  if (!match) return { path: trimmed, line: null }
  const line = Number(match[2])
  return { path: match[1], line: Number.isFinite(line) && line > 0 ? line : null }
}

interface ScoredPath {
  path: string
  score: number
}

/**
 * Rank a path against a query. Higher is better. `null` means no match.
 *
 * Tiered to mirror VSCode Quick Open intuitions:
 *  1. exact basename match           (best)
 *  2. basename startsWith(query)
 *  3. basename contains(query)
 *  4. path contains(query)
 *  5. subsequence fallback           (loosest)
 *
 * Within a tier shorter matches win — typing "foo" should rank
 * `foo.ts` above `subdir/foobarbaz.ts`.
 */
function scorePath(path: string, query: string): number | null {
  if (query.length === 0) return 0
  const pl = path.toLowerCase()
  const ql = query.toLowerCase()
  const slash = pl.lastIndexOf('/')
  const base = slash >= 0 ? pl.slice(slash + 1) : pl

  if (base === ql) return 1_000_000
  if (base.startsWith(ql)) return 900_000 - base.length
  if (base.includes(ql)) return 800_000 - base.length
  if (pl.includes(ql)) return 700_000 - pl.length

  // Skip the subsequence fallback for very short queries: a single
  // character is a subsequence of nearly every path, so the fallback
  // would score ~25k paths on the first keystroke.
  if (ql.length < 2) return null

  let i = 0
  for (let j = 0; j < pl.length && i < ql.length; j++) {
    if (pl[j] === ql[i]) i++
  }
  if (i === ql.length) return 500_000 - pl.length
  return null
}

function rankPaths(paths: string[], query: string): ScoredPath[] {
  if (query.length === 0) {
    // No query: surface the first N paths in their natural (sorted)
    // order so the empty palette shows something rather than blank.
    return paths.slice(0, MAX_RESULTS).map((path) => ({ path, score: 0 }))
  }
  const scored: ScoredPath[] = []
  for (const path of paths) {
    const score = scorePath(path, query)
    if (score !== null) scored.push({ path, score })
  }
  scored.sort((a, b) => b.score - a.score || a.path.localeCompare(b.path))
  return scored.slice(0, MAX_RESULTS)
}

/**
 * Build the palette groups shown when `/` mode is active. Returns one
 * "Files" group with up to MAX_RESULTS entries. Selecting an entry
 * opens the file as a non-temporary text tab, jumping to the parsed
 * line if a `:line` suffix was present.
 */
export function getFilePaletteGroups(projectId: string | null, rawQuery: string): CommandGroup[] {
  if (!projectId) return []

  const paths = getProjectFilePaths(projectId)
  if (paths.length === 0) return []

  const { path: pathQuery, line } = parseFileQuery(rawQuery)
  const ranked = rankPaths(paths, pathQuery)
  if (ranked.length === 0) return []

  const commands: Command[] = ranked.map(({ path }) => {
    const dir = dirname(path)
    const entry: Command = {
      id: `file:${path}`,
      label: basename(path),
      subtitle: dir || undefined,
      lucideIcon: 'file',
      keywords: [path],
      onSelect: () => {
        void openTextFile(projectId, path, {
          temporary: false,
          reveal: line !== null ? { kind: 'position', lineNumber: line, column: 1 } : undefined
        })
      }
    }
    return entry
  })

  return [{ heading: 'Files', commands }]
}
