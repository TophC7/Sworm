import { backend } from '$lib/api/backend'
import { refreshAllLspProjectEnvironments } from '$lib/features/editor/lsp/registry'
import { notify } from '$lib/features/notifications/state.svelte'
import type { SettingsDiagnostic } from '$lib/types/backend'

let diagnostics = $state<SettingsDiagnostic[]>([])
let listenerBooted = false
let lastDiagnosticKeys = new Set<string>()

export function getSettingsDiagnostics(): SettingsDiagnostic[] {
  return diagnostics
}

export async function refreshSettingsDiagnostics(projectPath?: string): Promise<void> {
  const payload = await backend.settings.getEffective(projectPath)
  setDiagnostics(payload.diagnostics)
}

export function ensureSettingsDiagnosticsListener(): void {
  if (listenerBooted) return
  listenerBooted = true
  backend.settings
    .onChanged((event) => {
      setDiagnostics(event.diagnostics)
      void refreshAllLspProjectEnvironments()
    })
    .catch((error) => {
      listenerBooted = false
      notify.error('Settings diagnostics stopped updating', error instanceof Error ? error.message : String(error))
    })
}

function setDiagnostics(next: SettingsDiagnostic[]): void {
  const nextKeyList = next.map(diagnosticKey)
  const isUnchanged =
    nextKeyList.length === diagnostics.length &&
    nextKeyList.every((key, index) => key === diagnosticKey(diagnostics[index]))
  if (isUnchanged) return

  const nextKeys = new Set(nextKeyList)
  const newDiagnostics = next.filter((diagnostic) => !lastDiagnosticKeys.has(diagnosticKey(diagnostic)))

  diagnostics = next
  lastDiagnosticKeys = nextKeys

  const firstNew = newDiagnostics[0]
  if (firstNew) {
    notify.error('Invalid settings file', `${firstNew.path}: ${firstNew.message}`)
  }
}

function diagnosticKey(diagnostic: SettingsDiagnostic): string {
  return [diagnostic.layer, diagnostic.path, diagnostic.pointer, diagnostic.code, diagnostic.message].join('\u0000')
}
