import { platform, requireNative } from '$lib/platform'
import { DND_MIME, parsePayload } from '$lib/features/dnd/payload'
import { LocalTransfer } from '$lib/features/dnd/transfer.svelte'
import * as modelCache from '$lib/features/editor/renderers/monaco/text/modelCache'
import { notify } from '$lib/features/notifications/state.svelte'
import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
import * as sessionRegistry from '$lib/features/sessions/terminal/sessionRegistry'
import * as taskRegistry from '$lib/features/tasks/taskRegistry'
import type { Tab } from '$lib/features/workbench/model'
import {
  getActiveTabId,
  getTabs,
  getWorkbenchId,
  finalizeTransferredTab,
  isTabTransferring,
  setTabTransferring,
  removeTransferredTab,
  setTaskTabStatus,
  stageTransferredTab
} from '$lib/features/workbench/state.svelte'
import type {
  TransferRequestEvent,
  TransferImportEvent,
  TransferCommittedEvent,
  TransferFinalizedEvent,
  TransferAbortedEvent
} from '$lib/platform'
import type { Unsubscribe } from '$lib/api/transport'

const sourceTransfers = new Map<string, string>()
const targetTransfers = new Map<string, { tab: Tab; staged: boolean }>()
const blockedEvents = ['beforeinput', 'keydown', 'paste', 'drop'] as const
let initialized: Promise<Unsubscribe[]> | null = null
let inputListenersActive = false

function updateInputBlocking(): void {
  const active = sourceTransfers.size > 0 || targetTransfers.size > 0
  if (active && !inputListenersActive) {
    for (const type of blockedEvents) globalThis.addEventListener(type, blockInput, true)
    inputListenersActive = true
  } else if (!active && inputListenersActive) {
    for (const type of blockedEvents) globalThis.removeEventListener(type, blockInput, true)
    inputListenersActive = false
  }
}

function blockInput(event: Event): void {
  if (!isTabTransferring(getActiveTabId() ?? '')) return
  // Tab strip stays usable so the user can switch away from the frozen tab.
  const target = event.target
  if (target instanceof Element && target.closest('[data-tab-id]')) return
  event.preventDefault()
  event.stopImmediatePropagation()
}

function setSourceTransfer(transferId: string, tabId: string): void {
  sourceTransfers.set(transferId, tabId)
  setTabTransferring(tabId, true)
  updateInputBlocking()
}

function clearSourceTransfer(transferId: string): void {
  const tabId = sourceTransfers.get(transferId)
  if (tabId) setTabTransferring(tabId, false)
  sourceTransfers.delete(transferId)
  updateInputBlocking()
}

function clearTargetTransfer(transferId: string): void {
  targetTransfers.delete(transferId)
  updateInputBlocking()
}

function cleanupRegistries(tab: Tab): void {
  if (tab.kind === 'session') sessionRegistry.detachForTransfer(tab.id)
  else if (tab.kind === 'task') taskRegistry.detachForTransfer(tab.runId)
  else if (tab.kind === 'text') modelCache.detachForTransfer(tab.id)
}

function isTab(value: unknown): value is Tab {
  if (!value || typeof value !== 'object') return false
  const tab = value as { id?: unknown; kind?: unknown; folderPath?: unknown }
  return (
    typeof tab.id === 'string' &&
    typeof tab.folderPath === 'string' &&
    typeof tab.kind === 'string' &&
    ['session', 'diff', 'text', 'tool', 'launcher', 'task', 'issue', 'epic'].includes(tab.kind)
  )
}

async function abortLocally(transferId: string, reason: string): Promise<void> {
  clearSourceTransfer(transferId)
  await requireNative()
    .transfers.abort(transferId, reason)
    .catch(() => {})
}

async function handleTransferRequest({ transferId, tabId }: TransferRequestEvent): Promise<void> {
  const tab = getTabs().find((candidate) => candidate.id === tabId)
  if (!tab) {
    await abortLocally(transferId, `Source tab ${tabId} no longer exists`)
    return
  }

  setSourceTransfer(transferId, tabId)
  try {
    let terminalState = null
    let modelState = null
    if (tab.kind === 'session') {
      terminalState = await sessionRegistry.exportTransferState(tab.id)
    } else if (tab.kind === 'task') {
      terminalState = await taskRegistry.exportTransferState(tab.runId)
    } else if (tab.kind === 'text') {
      modelState = modelCache.exportModelTransfer(tab.id)
    }

    await requireNative().transfers.sourceExported({ transferId, tab, terminalState, modelState })
  } catch (error) {
    await abortLocally(transferId, getErrorMessage(error))
  }
}

async function handleTransferImport({ transferId, exportPayload, targetIndex }: TransferImportEvent): Promise<void> {
  if (!isTab(exportPayload.tab)) {
    await requireNative()
      .transfers.abort(transferId, 'Invalid tab transfer payload')
      .catch(() => {})
    return
  }

  const tab = exportPayload.tab
  targetTransfers.set(transferId, { tab, staged: false })
  updateInputBlocking()
  try {
    // Monaco is browser-only; loading it statically makes SvelteKit's SSR evaluation fail.
    const monaco = exportPayload.modelState ? await import('monaco-editor') : null
    if (!targetTransfers.has(transferId)) {
      cleanupRegistries(tab)
      return
    }

    if (exportPayload.terminalState) {
      if (tab.kind === 'session') {
        await sessionRegistry.importTransferState(tab.id, exportPayload.terminalState, transferId)
        if (!targetTransfers.has(transferId)) {
          cleanupRegistries(tab)
          return
        }
      } else if (tab.kind === 'task') {
        await taskRegistry.importTransferState(
          {
            runId: tab.runId,
            attachOnly: tab.attachOnly,
            folderPath: tab.folderPath,
            taskId: tab.taskId,
            activeFilePath: tab.activeFilePath,
            onStatusChange: (status, exitCode) => setTaskTabStatus(tab.id, status, exitCode)
          },
          exportPayload.terminalState,
          transferId
        )
        if (!targetTransfers.has(transferId)) {
          cleanupRegistries(tab)
          return
        }
      }
    }
    if (exportPayload.modelState && monaco) modelCache.importModelTransfer(exportPayload.modelState, monaco)
    stageTransferredTab(tab, targetIndex)
    targetTransfers.get(transferId)!.staged = true
    await requireNative().transfers.targetStaged(transferId)
  } catch (error) {
    cleanupRegistries(tab)
    if (targetTransfers.get(transferId)?.staged) removeTransferredTab(tab.id)
    clearTargetTransfer(transferId)
    await requireNative()
      .transfers.abort(transferId, getErrorMessage(error))
      .catch(() => {})
  }
}

function handleTransferCommitted({ transferId, tabId }: TransferCommittedEvent): void {
  const tab = getTabs().find((candidate) => candidate.id === tabId)
  if (tab) cleanupRegistries(tab)
  removeTransferredTab(tabId)
  clearSourceTransfer(transferId)
}

function handleTransferFinalized({ transferId }: TransferFinalizedEvent): void {
  const target = targetTransfers.get(transferId)
  if (!target) return
  finalizeTransferredTab(target.tab.id)
  clearTargetTransfer(transferId)
}

function handleTransferAborted({ transferId, reason, ptyLost }: TransferAbortedEvent): void {
  const sourceTabId = sourceTransfers.get(transferId)
  if (sourceTabId) {
    const tab = getTabs().find((candidate) => candidate.id === sourceTabId)
    if (ptyLost) {
      if (tab?.kind === 'session') sessionRegistry.get(tab.id)?.markPtyLost()
      else if (tab?.kind === 'task') taskRegistry.get(tab.runId)?.markPtyLost()
    }
    clearSourceTransfer(transferId)
  }

  const target = targetTransfers.get(transferId)
  if (target) {
    cleanupRegistries(target.tab)
    if (target.staged) removeTransferredTab(target.tab.id)
    clearTargetTransfer(transferId)
  }

  if (ptyLost && sourceTabId) {
    notify.error('Tab transfer failed', 'The process connection was lost; the tab is marked failed.')
  } else if (reason === 'timeout') notify.warning('Tab transfer timed out; tab stayed in its original window.')
  else notify.warning('Tab transfer aborted', reason)
}

async function setupListeners(): Promise<Unsubscribe[]> {
  const transfers = requireNative().transfers
  const registrations = await Promise.allSettled([
    transfers.onRequest((event) => void handleTransferRequest(event)),
    transfers.onImport((event) => void handleTransferImport(event)),
    transfers.onCommitted(handleTransferCommitted),
    transfers.onFinalized(handleTransferFinalized),
    transfers.onAborted(handleTransferAborted)
  ])
  const failure = registrations.find((registration) => registration.status === 'rejected')
  if (failure?.status === 'rejected') {
    for (const registration of registrations) if (registration.status === 'fulfilled') registration.value()
    throw failure.reason
  }
  return registrations.map((registration) => (registration as PromiseFulfilledResult<Unsubscribe>).value)
}

export async function initTransferService(): Promise<() => void> {
  if (!platform.capabilities.tabTransfer) return () => {}
  initialized ??= setupListeners().catch((error) => {
    initialized = null
    throw error
  })
  const unlisten = await initialized
  return () => {
    for (const cleanup of unlisten) cleanup()
    for (const type of blockedEvents) globalThis.removeEventListener(type, blockInput, true)
    initialized = null
  }
}

/** True for a tab drag that originated in another window (same-window drags live in LocalTransfer). */
export function isForeignTabDrag(event: DragEvent): boolean {
  return (
    platform.capabilities.tabTransfer &&
    !LocalTransfer.has('tab') &&
    !!event.dataTransfer?.types.includes(DND_MIME.SWORM_TAB)
  )
}

/** Initiate a cross-window transfer from a drop event. */
export function dropForeignTab(event: DragEvent, targetIndex: number): boolean {
  if (!platform.capabilities.tabTransfer) return false
  const payload = parsePayload(event.dataTransfer?.getData(DND_MIME.SWORM_ITEM))
  const item = payload?.items.find((candidate) => candidate.kind === 'tab')
  const targetWindow = getWorkbenchId()
  if (!item?.sourceWindowLabel || item.sourceWindowLabel === targetWindow) return false

  event.preventDefault()
  void requireNative()
    .transfers.initiate({
      sourceWindow: item.sourceWindowLabel,
      targetWindow,
      tabId: item.tabId,
      targetIndex
    })
    .catch((error) => notify.warning('Tab transfer failed', getErrorMessage(error)))
  return true
}
