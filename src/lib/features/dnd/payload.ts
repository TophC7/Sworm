import type { TabId } from '$lib/features/workbench/model'

export const DND_MIME = {
  SWORM_ITEM: 'application/vnd.sworm.item+json',
  SWORM_TAB: 'application/vnd.sworm.kind.tab',
  SWORM_FILE: 'application/vnd.sworm.kind.file',
  SWORM_GIT_CHANGE: 'application/vnd.sworm.kind.git-change',
  TEXT: 'text/plain',
  URI_LIST: 'text/uri-list',
  FILES: 'Files'
} as const

export interface TabDragItem {
  kind: 'tab'
  tabId: TabId
  sourceWindowLabel?: string
}

export interface FileDragItem {
  kind: 'file'
  path: string
  isDir: boolean
  folderPath: string
}

export interface GitChangeDragItem {
  kind: 'git-change'
  path: string
  staged: boolean
  folderPath: string
}

export interface OsFilesDragItem {
  kind: 'os-files'
  paths: string[]
}

export type SwormDragKind = TabDragItem | FileDragItem | GitChangeDragItem | OsFilesDragItem

export interface DragPayload {
  items: SwormDragKind[]
  source: 'internal' | 'external'
}

export function serializePayload(payload: DragPayload): string {
  return JSON.stringify(payload)
}

export function parsePayload(raw: string | null | undefined): DragPayload | null {
  if (!raw) return null
  try {
    const parsed = JSON.parse(raw) as unknown
    return isDragPayload(parsed) ? parsed : null
  } catch {
    return null
  }
}

export function stampDataTransfer(dataTransfer: DataTransfer, payload: DragPayload): void {
  dataTransfer.setData(DND_MIME.SWORM_ITEM, serializePayload(payload))
  if (payload.items.some((item) => item.kind === 'tab')) {
    dataTransfer.setData(DND_MIME.SWORM_TAB, '')
  }
  if (payload.items.some((item) => item.kind === 'file')) {
    dataTransfer.setData(DND_MIME.SWORM_FILE, '')
  }
  if (payload.items.some((item) => item.kind === 'git-change')) {
    dataTransfer.setData(DND_MIME.SWORM_GIT_CHANGE, '')
  }
  const text = payloadText(payload)
  if (text) {
    dataTransfer.setData(DND_MIME.TEXT, text)
  }
}

export function dragTypes(event: DragEvent): readonly string[] {
  return Array.from(event.dataTransfer?.types ?? [])
}

export function hasKnownDragType(types: readonly string[]): boolean {
  return (
    types.includes(DND_MIME.SWORM_ITEM) ||
    types.includes(DND_MIME.SWORM_TAB) ||
    types.includes(DND_MIME.SWORM_FILE) ||
    types.includes(DND_MIME.SWORM_GIT_CHANGE) ||
    types.includes(DND_MIME.URI_LIST) ||
    types.includes(DND_MIME.FILES) ||
    types.includes(DND_MIME.TEXT)
  )
}

export function payloadHasKind(payload: DragPayload | null, kind: SwormDragKind['kind']): boolean {
  return payload?.items.some((item) => item.kind === kind) ?? false
}

function payloadText(payload: DragPayload): string {
  const first = payload.items[0]
  if (!first) return ''
  // Deliberately skip text/plain for tab drags. Monaco's drop handler reads
  // text/plain and inserts it at the caret, so stamping the tab id here
  // would make "tab-xxxx-N$0" appear inside the editor on drop.
  if (first.kind === 'tab') return ''
  if (first.kind === 'file' || first.kind === 'git-change') return first.path
  if (first.kind === 'os-files') return first.paths[0] ?? ''
  return ''
}

function isDragPayload(value: unknown): value is DragPayload {
  if (!value || typeof value !== 'object') return false
  const payload = value as Partial<DragPayload>
  if (payload.source !== 'internal' && payload.source !== 'external') return false
  if (!Array.isArray(payload.items)) return false
  return payload.items.every(isDragItem)
}

function isDragItem(value: unknown): value is SwormDragKind {
  if (!value || typeof value !== 'object') return false
  const item = value as Partial<SwormDragKind>
  if (item.kind === 'tab') {
    return (
      typeof item.tabId === 'string' &&
      (item.sourceWindowLabel === undefined || typeof item.sourceWindowLabel === 'string')
    )
  }
  if (item.kind === 'file') {
    return typeof item.path === 'string' && typeof item.isDir === 'boolean' && typeof item.folderPath === 'string'
  }
  if (item.kind === 'git-change') {
    return typeof item.path === 'string' && typeof item.staged === 'boolean' && typeof item.folderPath === 'string'
  }
  if (item.kind === 'os-files') {
    return Array.isArray(item.paths) && item.paths.every((path) => typeof path === 'string')
  }
  return false
}
