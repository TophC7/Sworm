// Global UI state module using Svelte 5 runes.
//
// Stores visual preferences that are NOT per-project:
// sidebar dimensions, collapse state, zoom level, window controls.

export type SidebarView = 'git' | 'sessions' | 'files'

let sidebarWidth = $state(280)
let sidebarCollapsed = $state(false)
let sidebarView = $state<SidebarView>('files')
let zoomLevel = $state(1.0)

// ---------------------------------------------------------------------------
// Window controls — persisted to localStorage
// ---------------------------------------------------------------------------

export interface WindowControlsConfig {
  useSystemDecorations: boolean
  showMinimize: boolean
  showMaximize: boolean
  showClose: boolean
}

const WC_STORAGE_KEY = 'sworm:windowControls'

function loadWindowControls(): WindowControlsConfig {
  const defaults: WindowControlsConfig = {
    useSystemDecorations: false,
    showMinimize: true,
    showMaximize: true,
    showClose: true
  }
  if (typeof localStorage === 'undefined') return defaults
  try {
    const raw = localStorage.getItem(WC_STORAGE_KEY)
    if (raw) return { ...defaults, ...JSON.parse(raw) }
  } catch {
    /* ignore corrupt data */
  }
  return defaults
}

function persistWindowControls(config: WindowControlsConfig) {
  if (typeof localStorage === 'undefined') return
  localStorage.setItem(WC_STORAGE_KEY, JSON.stringify(config))
}

let windowControls = $state<WindowControlsConfig>(loadWindowControls())

export function getWindowControls(): WindowControlsConfig {
  return windowControls
}

export function setWindowControls(patch: Partial<WindowControlsConfig>) {
  windowControls = { ...windowControls, ...patch }
  persistWindowControls(windowControls)
}

// ---------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------

export function getSidebarWidth(): number {
  return sidebarWidth
}

export function setSidebarWidth(width: number) {
  sidebarWidth = Math.max(220, Math.min(520, width))
}

export function isSidebarCollapsed(): boolean {
  return sidebarCollapsed
}

export function setSidebarCollapsed(collapsed: boolean) {
  sidebarCollapsed = collapsed
}

export function toggleSidebar() {
  sidebarCollapsed = !sidebarCollapsed
}

// ---------------------------------------------------------------------------
// Sidebar view
// ---------------------------------------------------------------------------

export function getSidebarView(): SidebarView {
  return sidebarView
}

export function setSidebarView(view: SidebarView) {
  sidebarView = view
}

// Zoom — uses native webview zoom so px-based icons scale correctly //

import { getCurrentWebview } from '@tauri-apps/api/webview'

let zoomTimer: ReturnType<typeof setTimeout> | undefined

function applyZoom() {
  clearTimeout(zoomTimer)
  zoomTimer = setTimeout(() => {
    getCurrentWebview()
      .setZoom(zoomLevel)
      .catch(() => {})
  }, 80)
}

export function getZoomLevel(): number {
  return zoomLevel
}

export function setZoomLevel(level: number) {
  zoomLevel = Math.round(Math.max(0.5, Math.min(2.0, level)) * 10) / 10
  applyZoom()
}

export function zoomIn() {
  setZoomLevel(zoomLevel + 0.1)
}

export function zoomOut() {
  setZoomLevel(zoomLevel - 0.1)
}

export function zoomReset() {
  setZoomLevel(1.0)
}

// ---------------------------------------------------------------------------
// Command palette
// ---------------------------------------------------------------------------

let commandPaletteOpen = $state(false)

export function isCommandPaletteOpen(): boolean {
  return commandPaletteOpen
}

export function setCommandPaletteOpen(open: boolean) {
  commandPaletteOpen = open
}

export function toggleCommandPalette() {
  commandPaletteOpen = !commandPaletteOpen
}
