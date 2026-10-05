import { SvelteMap } from 'svelte/reactivity'
import { platform, requireNative } from '$lib/platform'
import { DND_MIME, parsePayload } from '$lib/features/dnd/payload'
import * as modelCache from '$lib/features/editor/renderers/monaco/text/modelCache'
import { notify } from '$lib/features/notifications/state.svelte'
import { getErrorMessage } from '$lib/utils/client-error'
import * as sessionRegistry from '$lib/features/sessions/terminal/sessionRegistry'
import * as taskRegistry from '$lib/features/tasks/taskRegistry'
import { isTab, tabServer, type Tab, type TabId } from '$lib/features/workbench/model'
import {
  getActiveTabId,
  getTabs,
  finalizeTransferredTab,
  removeTransferredTab,
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
import { requestGroup } from './groups.svelte'

const sourceTransfers = new Map<string, TabId>()
const targetTransfers = new Map<string, { tab: Tab; staged: boolean }>()
const frozen = new SvelteMap<TabId, string>()
const blockedEvents = ['beforeinput', 'keydown', 'paste', 'drop'] as const
let initialized: Promise<Unsubscribe[]> | null = null
let inputListenersActive = false

function updateInputBlocking(): void {
  const active = frozen.size > 0 || targetTransfers.size > 0
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

function setSourceTransfer(transferId: string, tabId: TabId): void {
  sourceTransfers.set(transferId, tabId)
  freezeTabs(transferId, [tabId])
}

function clearSourceTransfer(transferId: string): void {
  sourceTransfers.delete(transferId)
  freezeTabs(transferId, null)
}

function clearTargetTransfer(transferId: string): void {
  targetTransfers.delete(transferId)
  freezeTabs(transferId, null)
}
export function isTabTransferring(tabId: TabId): boolean {
  return frozen.has(tabId)
}

/** Freeze source/staged input without discarding its views or dirty models. */
export function freezeTabs(transferId: string, tabIds: readonly TabId[] | null): void {
  for (const [tabId, owner] of frozen) {
    if (owner === transferId) frozen.delete(tabId)
  }
  if (tabIds) for (const tabId of tabIds) frozen.set(tabId, transferId)
  updateInputBlocking()
}

export function cleanupRegistries(tab: Tab): void {
  if (tab.kind === 'session') sessionRegistry.release(tab.id)
  else if (tab.kind === 'task') taskRegistry.release(tab.runId)
  else if (tab.kind === 'text') modelCache.detachForTransfer(tab)
}


async function abortLocally(transferId: string, reason: string): Promise<void> {
  clearSourceTransfer(transferId)
  await requireNative()
    .transfers.abort(transferId, reason)
    .catch(() => {})
}

/** Server-workbench tabs move only with their whole group. */
async function rejectServerTab(transferId: string, tab: Tab): Promise<boolean> {
  if (tabServer(tab) === null) return false
  notify.warning('Move the whole workbench', 'Tabs in a server workbench cannot move individually between windows.')
  await abortLocally(transferId, 'Move the whole workbench instead of an individual tab')
  return true
}

async function handleTransferRequest({ transferId, tabId }: TransferRequestEvent): Promise<void> {
  const tab = getTabs().find((candidate) => candidate.id === tabId)
  if (!tab) {
    await abortLocally(transferId, `Source tab ${tabId} no longer exists`)
    return
  }
  if (await rejectServerTab(transferId, tab)) return

  setSourceTransfer(transferId, tabId)
  try {
    let terminalState = null
    let modelState = null
    if (tab.kind === 'session') {
      terminalState = await sessionRegistry.exportTransferState(tab.id)
    } else if (tab.kind === 'task') {
      terminalState = await taskRegistry.exportTransferState(tab.runId)
    } else if (tab.kind === 'text') {
      modelState = modelCache.exportModelTransfer(tab)
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
  if (await rejectServerTab(transferId, tab)) return
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
          tab,
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
    freezeTabs(transferId, [tab.id])
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

async function setupListeners(transfers: NonNullable<typeof platform.native>['transfers']): Promise<Unsubscribe[]> {
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
  const native = platform.native
  if (!native) return () => {}
  initialized ??= setupListeners(native.transfers).catch((error) => {
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


/** Accept a tab or a whole group dragged in from another window. */
export function dropFromOtherWindow(event: DragEvent, targetIndex: number): boolean {
  const native = platform.native
  if (!native) return false
  const item = parsePayload(event.dataTransfer?.getData(DND_MIME.SWORM_ITEM))?.items[0]
  const targetWindow = platform.workbench.id
  if (
    (item?.kind !== 'tab' && item?.kind !== 'workbench') ||
    !item.sourceWindowLabel ||
    item.sourceWindowLabel === targetWindow
  )
    return false

  event.preventDefault()
  const request =
    item.kind === 'workbench'
      ? requestGroup(item.sourceWindowLabel, item.server, item.workbenchId, targetIndex)
      : native.transfers.initiate({
          sourceWindow: item.sourceWindowLabel,
          targetWindow,
          tabId: item.tabId,
          targetIndex
        })
  void request.catch((error) =>
    notify.warning(item.kind === 'workbench' ? 'Move workbench failed' : 'Tab transfer failed', getErrorMessage(error))
  )
  return true
}
