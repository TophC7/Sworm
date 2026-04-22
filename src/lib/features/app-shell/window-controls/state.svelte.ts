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
