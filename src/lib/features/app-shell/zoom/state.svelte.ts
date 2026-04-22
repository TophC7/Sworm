import { getCurrentWebview } from '@tauri-apps/api/webview'

let zoomLevel = $state(1.0)
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
