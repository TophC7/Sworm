// Filter DSL for the issues sidebar.
//
// Free text matches id/title. Operators:
//   is:open|active|done|archived
//   p:N | p:N..M
//   epic:ID
//
// Same-kind tokens combine with OR (clicking is:open then is:done yields
// the union). Cross-kind tokens AND together. The query string itself is
// the single source of truth for "what's visible". UI toggles literal
// tokens via toggleToken / hasToken instead of owning parallel state.

import type { Issue, IssueStatus } from '$lib/types/backend'
import { isOpen } from './visual'

export type Term =
  | { kind: 'is'; value: 'open' | 'active' | 'done' | 'archived' }
  | { kind: 'p'; min: number; max: number }
  | { kind: 'epic'; id: string }
  | { kind: 'text'; value: string }

export function splitTokens(query: string): string[] {
  return query.split(/\s+/).filter(Boolean)
}

export function hasToken(query: string, token: string): boolean {
  const lower = token.toLowerCase()
  return splitTokens(query).some((t) => t.toLowerCase() === lower)
}

export function toggleToken(query: string, token: string): string {
  const tokens = splitTokens(query)
  const lower = token.toLowerCase()
  const idx = tokens.findIndex((t) => t.toLowerCase() === lower)
  if (idx >= 0) tokens.splice(idx, 1)
  else tokens.push(token)
  return tokens.join(' ')
}

export function parseQuery(raw: string): Term[] {
  const out: Term[] = []
  for (const tok of raw.toLowerCase().split(/\s+/).filter(Boolean)) {
    if (tok.startsWith('is:')) {
      const v = tok.slice(3)
      if (v === 'open' || v === 'active' || v === 'done' || v === 'archived') {
        out.push({ kind: 'is', value: v })
        continue
      }
    }
    if (tok.startsWith('p:')) {
      const m = tok.slice(2).match(/^(\d+)(?:\.\.(\d+))?$/)
      if (m) {
        const min = Number(m[1])
        const max = m[2] !== undefined ? Number(m[2]) : min
        out.push({ kind: 'p', min, max })
        continue
      }
    }
    if (tok.startsWith('epic:')) {
      out.push({ kind: 'epic', id: tok.slice(5) })
      continue
    }
    out.push({ kind: 'text', value: tok })
  }
  return out
}

function matchesIs(status: IssueStatus, value: 'open' | 'active' | 'done' | 'archived'): boolean {
  if (value === 'open') return isOpen(status)
  if (value === 'active') return status === 'in_progress'
  if (value === 'done') return status === 'completed'
  return status === 'archived'
}

export function matchesIssue(issue: Issue, terms: Term[]): boolean {
  for (const t of terms) {
    if (t.kind === 'text') {
      const hay = `${issue.id} ${issue.title}`.toLowerCase()
      if (!hay.includes(t.value)) return false
    } else if (t.kind === 'epic') {
      if (!issue.epicId || !issue.epicId.toLowerCase().includes(t.id)) return false
    }
  }
  const ps = terms.filter((t): t is Extract<Term, { kind: 'p' }> => t.kind === 'p')
  if (ps.length > 0 && !ps.some((t) => issue.priority >= t.min && issue.priority <= t.max)) return false
  const iss = terms.filter((t): t is Extract<Term, { kind: 'is' }> => t.kind === 'is')
  if (iss.length > 0 && !iss.some((t) => matchesIs(issue.status, t.value))) return false
  return true
}
