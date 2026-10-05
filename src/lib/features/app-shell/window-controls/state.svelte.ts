import { platform } from '$lib/platform'

export interface WindowControlsConfig {
  useSystemDecorations: boolean
  showMinimize: boolean
  showMaximize: boolean
  showClose: boolean
}

const WC_STORAGE_KEY = 'sworm:window-controls'
const LEGACY_WC_STORAGE_KEY = 'sworm:windowControls'

function loadWindowControls(): WindowControlsConfig {
  const defaults: WindowControlsConfig = {
    useSystemDecorations: false,
    showMinimize: true,
    showMaximize: true,
    showClose: true
  }
  if (typeof localStorage === 'undefined') return defaults
  try {
    const raw = localStorage.getItem(WC_STORAGE_KEY) ?? localStorage.getItem(LEGACY_WC_STORAGE_KEY)
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

function applyDecorations(enabled: boolean): void {
  const native = platform.native
  if (!native) return
  void native.window.setDecorations(enabled).catch((error) => {
    console.warn('Failed to set window decorations:', error)
  })
}

export function initWindowControls(): () => void {
  if (windowControls.useSystemDecorations) applyDecorations(true)
  const onStorage = (event: StorageEvent) => {
    if (event.key !== WC_STORAGE_KEY) return
    windowControls = loadWindowControls()
    applyDecorations(windowControls.useSystemDecorations)
  }
  window.addEventListener('storage', onStorage)
  return () => window.removeEventListener('storage', onStorage)
}

export function getWindowControls(): WindowControlsConfig {
  return windowControls
}

export function setWindowControls(patch: Partial<WindowControlsConfig>) {
  windowControls = { ...windowControls, ...patch }
  persistWindowControls(windowControls)
  if (patch.useSystemDecorations !== undefined) applyDecorations(patch.useSystemDecorations)
}
