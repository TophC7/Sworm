import { requireNative } from '$lib/platform'

let zoomLevel = $state(1.0)
let zoomTimer: ReturnType<typeof setTimeout> | undefined

export function getZoomLevel(): number {
  return zoomLevel
}

export function setZoomLevel(level: number) {
  const zoom = requireNative().zoom
  zoomLevel = Math.round(Math.max(0.5, Math.min(2.0, level)) * 10) / 10
  clearTimeout(zoomTimer)
  zoomTimer = setTimeout(() => {
    void zoom.setZoom(zoomLevel).catch(() => {})
  }, 80)
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
