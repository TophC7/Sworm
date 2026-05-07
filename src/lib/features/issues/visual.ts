// Visual helpers for the issues feature: status glyphs/colors and
// priority styling. Shared between the sidebar list and the editor-tab
// surface so both speak the same visual vocabulary.

import type { IssueStatus } from '$lib/types/backend'

const STATUS_GLYPH: Record<IssueStatus, string> = {
  todo: '◇',
  in_progress: '●',
  blocked: '⊘',
  completed: '✓',
  wont_fix: '✗',
  archived: '⌫'
}

const STATUS_TONE: Record<IssueStatus, string> = {
  todo: 'text-muted',
  in_progress: 'text-accent',
  blocked: 'text-warning',
  completed: 'text-success',
  wont_fix: 'text-danger',
  archived: 'text-subtle'
}

export function statusGlyph(status: IssueStatus): string {
  return STATUS_GLYPH[status] ?? '·'
}

export function statusGlyphTone(status: IssueStatus): string {
  return STATUS_TONE[status] ?? 'text-muted'
}

export function statusLabel(status: string): string {
  return status.replaceAll('_', ' ')
}

// Priority weight rises as the number falls. P0 is critical; tail priorities
// fade into the surface so the eye lands on what's hot.
export function priorityToneClass(priority: number): string {
  if (priority <= 0) return 'text-danger-bright font-bold'
  if (priority === 1) return 'text-warning font-semibold'
  if (priority === 2) return 'text-accent font-medium'
  if (priority === 3) return 'text-fg'
  return 'text-subtle'
}

// Active group ordering. Filtered/excluded statuses (archived) handled
// at the call site so toggles can flip them in.
export const STATUS_GROUP_ORDER: IssueStatus[] = ['in_progress', 'todo', 'blocked', 'completed', 'wont_fix', 'archived']

export const ALL_STATUSES: IssueStatus[] = ['todo', 'in_progress', 'blocked', 'completed', 'wont_fix', 'archived']

export const ALL_PRIORITIES: number[] = [0, 1, 2, 3, 4, 5]

const TERMINAL: ReadonlySet<IssueStatus> = new Set(['completed', 'wont_fix', 'archived'])

export function isOpen(status: IssueStatus): boolean {
  return !TERMINAL.has(status)
}

const STATUS_GROUP_LABEL: Record<IssueStatus, string> = {
  in_progress: 'Active',
  todo: 'Todo',
  blocked: 'Blocked',
  completed: 'Done',
  wont_fix: "Won't fix",
  archived: 'Archived'
}

export function statusGroupLabel(status: IssueStatus): string {
  return STATUS_GROUP_LABEL[status] ?? status
}

// Row-level status labels used in tooltips. "In progress" reads better
// inline than the group's "Active".
const STATUS_LABEL: Record<IssueStatus, string> = {
  todo: 'Todo',
  in_progress: 'In progress',
  blocked: 'Blocked',
  completed: 'Done',
  wont_fix: "Won't fix",
  archived: 'Archived'
}

export function statusRowLabel(status: IssueStatus): string {
  return STATUS_LABEL[status] ?? status
}
