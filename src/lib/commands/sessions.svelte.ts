import type { Command, CommandConfirm, CommandGroup } from './types'
import { allProviders, directOptions } from '$lib/data/providers'
import { getConnectedProviders } from '$lib/stores/providers.svelte'
import { createAndOpenSession, hasRunningSessions } from '$lib/stores/sessions.svelte'
import { ensureFreshSession } from '$lib/surfaces/session/service.svelte'
import { getActiveProjectId } from '$lib/workbench/state.svelte'
import { runNotifiedTask } from '$lib/utils/notifiedTask'

const freshIcon = directOptions.find((p) => p.id === 'fresh')?.icon ?? ''
const terminalIcon = directOptions.find((p) => p.id === 'terminal')?.icon ?? ''

// Shared workspace warning state
let warningOpen = $state(false)
let pendingCreate = $state<{ providerId: string; label: string } | null>(null)

const sessionWarningConfirm: CommandConfirm = {
  title: 'Shared Workspace Warning',
  message:
    'Another session is already running in this project.\n\n' +
    'Sessions in the same project share the same working tree and branch.\n' +
    'Changes made by one session may conflict with another.',
  confirmLabel: 'Start Anyway',
  isOpen: () => warningOpen,
  onConfirm: () => {
    warningOpen = false
    if (pendingCreate) {
      void doCreate(pendingCreate.providerId, pendingCreate.label)
      pendingCreate = null
    }
  },
  onCancel: () => {
    warningOpen = false
    pendingCreate = null
  }
}

function startSession(providerId: string, label: string) {
  const projectId = getActiveProjectId()
  if (!projectId) return
  if (hasRunningSessions()) {
    pendingCreate = { providerId, label }
    warningOpen = true
    return
  }
  void doCreate(providerId, label)
}

async function doCreate(providerId: string, label: string) {
  const projectId = getActiveProjectId()
  if (!projectId) return
  await runNotifiedTask(() => createAndOpenSession(projectId, providerId, `${label} session`), {
    loading: { title: `Starting ${label} session` },
    error: { title: `Failed to start ${label} session` }
  })
}

function openFresh() {
  const projectId = getActiveProjectId()
  if (!projectId) return
  void ensureFreshSession(projectId)
}

export function getSessionCommands(): CommandGroup[] {
  const activeId = getActiveProjectId()
  if (!activeId) return []

  const connected = getConnectedProviders()
  const connectedIds = new Set(connected.map((p) => p.id))
  const commands: Command[] = []

  for (const provider of allProviders) {
    if (!connectedIds.has(provider.id)) continue
    commands.push({
      id: `new-session-${provider.id}`,
      label: `New ${provider.label} Session`,
      iconSrc: provider.icon,
      keywords: ['new', 'session', 'agent', provider.label],
      onSelect: () => startSession(provider.id, provider.label)
    })
  }

  if (connectedIds.has('fresh')) {
    commands.push({
      id: 'open-fresh',
      label: 'Open Fresh',
      iconSrc: freshIcon,
      keywords: ['fresh', 'editor', 'text'],
      onSelect: openFresh
    })
  }

  if (connectedIds.has('terminal')) {
    commands.push({
      id: 'new-terminal',
      label: 'New Terminal',
      iconSrc: terminalIcon,
      keywords: ['terminal', 'shell', 'console'],
      shortcut: 'Ctrl+T',
      onSelect: () => startSession('terminal', 'Terminal')
    })
  }

  // Attach shared warning confirm to first command (deduplicated by reference in CommandCenter)
  if (commands.length > 0) {
    commands[0] = { ...commands[0], confirm: sessionWarningConfirm }
  }

  return commands.length > 0 ? [{ heading: 'Sessions', commands }] : []
}
