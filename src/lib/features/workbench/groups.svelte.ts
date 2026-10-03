import { backend } from '$lib/api/backend'
import type { AttachMode, WorkbenchAttached } from '$lib/types/backend'
import { notify } from '$lib/features/notifications/state.svelte'
import { cleanupRegistries, isTab, setGroupTransferring } from './transferService.svelte'
import * as modelCache from '$lib/features/editor/renderers/monaco/text/modelCache'
import { platform, requireNative, type GroupHandoff, type GroupImport, type GroupFinalized } from '$lib/platform'
import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
import { tabServer, type PersistedGroupRef, type PersistedTab, type PersistedWorkbenchV4, type Tab } from './model'
import {
  createWorkbenchWriter,
  parsePersistedWorkbench,
  persistedToTab,
  serializeWorkbench,
  type WorkbenchWriter
} from './persistence'
import {
  generateTabId,
  getActiveTabId,
  finalizeTransferredTab,
  getTabs,
  getWorkbenchId,
  persistWorkbench,
  removeTransferredTab,
  stageTransferredTab,
  replaceGroupTabs,
  setActiveTab
} from './state.svelte'
import { splitRemotePath } from '$lib/utils/paths'

export type GroupState = 'attaching' | 'active' | 'busy' | 'revoked' | 'offline'
export interface WorkbenchGroup {
  server: string
  id: string
  state: GroupState
  client: string | null
}

interface OwnedGroup extends WorkbenchGroup {
  token: string | null
  attachmentId: string | null
  writer: WorkbenchWriter | null
  generation: number
  index: number
  lastActiveTabId: string | null
  detaching: boolean
}

let groups = $state<OwnedGroup[]>([])
let listening: Promise<void> | null = null
let restoring = false
const groupTransfers = new Map<
  string,
  { group: OwnedGroup; source: boolean; tabs: Tab[]; previousActive: string | null }
>()

/** Groups this window tracks; busy/revoked groups own no tabs and render as a lone chip. */
export function getGroups(): WorkbenchGroup[] {
  return groups
}
function findGroup(server: string): OwnedGroup | undefined {
  return groups.find((group) => group.server === server)
}
export function getTabGroup(tab: Tab): WorkbenchGroup | null {
  const server = tabServer(tab)
  return server ? (findGroup(server) ?? null) : null
}
export function isTabInert(tab: Tab): boolean {
  const group = findGroup(tabServer(tab) ?? '')
  return !!group && (group.state !== 'active' || group.detaching)
}

function currentTabs(server: string): Tab[] {
  return getTabs().filter((tab) => tabServer(tab) === server)
}

/** Only folderPath is absolute in persisted tabs; filePath, activeFilePath, initialFile and scopePath are folder-relative. */
function mapGroupSnapshot(
  snapshot: PersistedWorkbenchV4,
  server: string,
  direction: 'toServer' | 'toWindow'
): PersistedWorkbenchV4 {
  return {
    ...snapshot,
    tabs: snapshot.tabs.map((tab): PersistedTab => {
      const remote = splitRemotePath(tab.folderPath)
      if (direction === 'toServer') {
        if (remote?.server !== server) throw new Error(`Tab folder does not belong to ${server}`)
        return { ...tab, folderPath: remote.path }
      }
      if (!tab.folderPath.startsWith('/')) throw new Error('Invalid server-local tab folder')
      return { ...tab, folderPath: `sworm://${server}${tab.folderPath}` }
    })
  }
}

function groupSnapshot(server: string): PersistedWorkbenchV4 {
  const group = findGroup(server)
  return mapGroupSnapshot(
    serializeWorkbench({
      tabs: currentTabs(server),
      activeTabId: group?.lastActiveTabId ?? getActiveTabId()
    }),
    server,
    'toServer'
  )
}

function addGroup(server: string, id: string, token: string | null, index = getTabs().length): OwnedGroup {
  const group: OwnedGroup = {
    server,
    id,
    token,
    attachmentId: null,
    state: 'attaching',
    client: null,
    generation: 0,
    index,
    lastActiveTabId: getActiveTabId(),
    writer: null,
    detaching: false
  }
  groups = [...groups, group]
  return groups[groups.length - 1]
}

function abandon(group: OwnedGroup): void {
  group.generation++
  group.writer?.stop()
  group.writer = null
  groups = groups.filter((candidate) => candidate !== group)
}

/** Controlled elsewhere: this window cannot know that owner's tabs, so it keeps none. */
async function yieldControl(group: OwnedGroup, state: 'busy' | 'revoked', client: string | null): Promise<boolean> {
  const generation = ++group.generation
  group.state = state
  group.client = client
  group.attachmentId = null
  group.writer?.stop()
  group.writer = null
  await releaseViews(group)
  if (group.generation !== generation) return false
  if (currentTabs(group.server).length) replaceGroupTabs(group.server, [])
  return true
}

async function releaseViews(group: OwnedGroup): Promise<void> {
  const runIds: string[] = []
  for (const tab of currentTabs(group.server)) {
    cleanupRegistries(tab)
    if (tab.kind === 'session' && tab.runId) runIds.push(tab.runId)
    else if (tab.kind === 'task') runIds.push(tab.runId)
  }
  if (!runIds.length) return
  try {
    await requireNative().remotes.releaseRuns(runIds)
  } catch (error) {
    console.warn('Workbench run release failed:', error)
  }
}

function hydrate(group: OwnedGroup, json: string, index?: number): void {
  const parsed = parsePersistedWorkbench(JSON.parse(json))
  if (!parsed) throw new Error('Malformed server workbench snapshot')
  const snapshot = mapGroupSnapshot(parsed, group.server, 'toWindow')
  const tabs = snapshot.tabs.map((tab) => persistedToTab(tab, generateTabId()))
  group.lastActiveTabId = tabs[snapshot.activeTabIndex]?.id ?? null
  replaceGroupTabs(group.server, tabs, index, snapshot.activeTabIndex)
}

function bindAttachment(group: OwnedGroup, attached: Extract<WorkbenchAttached, { kind: 'ready' }>): void {
  group.attachmentId = attached.attachment_id
  group.token = attached.controller_token
  group.client = null
  group.writer ??= createWorkbenchWriter((json) => backend.workbenches.save(group.server, group.id, json))
}

function detachAttachment(group: OwnedGroup, attachmentId = group.attachmentId): Promise<void> {
  return attachmentId ? backend.workbenches.detach(group.server, group.id, attachmentId) : Promise.resolve()
}

async function attach(
  group: OwnedGroup,
  takeover: boolean,
  hydrateSnapshot: boolean,
  index?: number,
  throwOnError = false
): Promise<string | null> {
  const generation = ++group.generation
  const attachmentId = crypto.randomUUID()
  group.attachmentId = attachmentId
  group.state = 'attaching'
  try {
    // A takeover proves nothing, so it never carries a token; a held token resumes.
    const attachMode: AttachMode = takeover
      ? { kind: 'takeover' }
      : group.token
        ? { kind: 'resume', controller_token: group.token }
        : { kind: 'open' }
    const result = await backend.workbenches.attach(group.server, group.id, attachMode, attachmentId)
    if (group.generation !== generation || !groups.includes(group)) {
      if (result?.kind === 'ready') await detachAttachment(group, result.attachment_id)
      return null
    }
    if (!result) {
      abandon(group)
      return null
    }
    if (result.kind === 'ready') {
      bindAttachment(group, result)
      if (hydrateSnapshot || !currentTabs(group.server).length) {
        await releaseViews(group)
        if (group.generation !== generation || !groups.includes(group)) {
          await detachAttachment(group, result.attachment_id)
          return null
        }
        if (index === undefined) hydrate(group, result.snapshot)
      }
      group.state = 'active'
      if (index === undefined) {
        group.writer!.schedule(() => groupSnapshot(group.server))
        persistWorkbench()
        if (!currentTabs(group.server).length) syncGroups()
        void refreshControl(group.server)
      }
    } else if (!(await yieldControl(group, result.kind, result.client))) return null
    return index === undefined || result.kind !== 'ready' ? null : result.snapshot
  } catch (error) {
    if (group.generation !== generation) return null
    group.state = 'offline'
    console.warn('Workbench attach failed:', error)
    if (throwOnError) throw error
    return null
  }
}

/** Await listeners before advertising window readiness to the handoff coordinator. */
export function initGroupService(): Promise<void> {
  if (!platform.capabilities.remoteHosts) return Promise.resolve()
  listening ??= listen().catch((error) => {
    listening = null
    throw error
  })
  return listening
}

async function refreshControl(server: string): Promise<void> {
  const group = findGroup(server)
  if (!group || group.state !== 'active' || group.detaching) return
  const generation = group.generation
  try {
    const entries = await backend.workbenches.list(server)
    if (group.generation !== generation || group.state !== 'active' || group.detaching) return
    const record = entries.find((entry) => entry.id === group.id)
    if (record?.yours) return
    if (!(await yieldControl(group, 'revoked', record?.client ?? null)) || record) return
    abandon(group)
    persistWorkbench()
    notify.warning('Workbench closed elsewhere', `${server} workbench was closed.`)
  } catch (error) {
    console.warn('Workbench status check failed:', error)
  }
}

async function listen(): Promise<void> {
  const native = requireNative()
  const registrations = await Promise.allSettled([
    backend.workbenches.onChanged(({ server }) => {
      if (server) void refreshControl(server)
    }),
    native.remotes.onStatus(({ server, connected }) => {
      const group = findGroup(server)
      if (!group) return
      if (!connected && (group.state === 'active' || group.state === 'attaching')) {
        group.state = 'offline'
        group.generation++
      } else if (connected && group.state === 'offline' && !group.detaching) {
        void attach(group, false, false)
      }
    }),
    native.window.onGroupRequest((handoff) => void exportGroup(handoff)),
    native.window.onGroupImport((handoff) => void stageGroup(handoff)),
    native.window.onGroupCommitted(commitGroup),
    native.window.onGroupFinalized(finalizeGroup),
    native.window.onGroupAborted(abortGroup),
    native.window.onGroupSettled((handoff) => {
      if (!handoff.committed) abortGroup(handoff)
      else if (groupTransfers.get(handoff.transferId)?.source) commitGroup(handoff)
      else finalizeGroup(handoff)
    })
  ])
  const failure = registrations.find((registration) => registration.status === 'rejected')
  if (failure?.status === 'rejected') {
    for (const registration of registrations) if (registration.status === 'fulfilled') registration.value()
    throw failure.reason
  }
}

/** Called by the state commit choke point, after the new tab list is installed. */
export function syncGroups(): void {
  if (!platform.capabilities.remoteHosts || restoring) return
  void initGroupService().catch((error) => console.warn('Workbench listeners failed:', error))
  const servers = new Set(
    getTabs()
      .map(tabServer)
      .filter((s): s is string => s !== null)
  )
  for (const group of [...groups]) {
    if (!servers.has(group.server)) {
      if (group.state === 'active' && !group.detaching) {
        group.detaching = true
        // Save emptiness before detaching so the server can discard an emptied workbench.
        const writer = group.writer!
        writer.schedule(() => serializeWorkbench({ tabs: [], activeTabId: null }))
        void (async () => {
          await writer.flush()
          if (!groups.includes(group) || group.state !== 'active' || currentTabs(group.server).length) return
          await detachAttachment(group)
          abandon(group)
          persistWorkbench()
          if (currentTabs(group.server).length) syncGroups()
        })()
          .catch((error) => console.warn('Workbench detach failed:', error))
          .finally(() => {
            group.detaching = false
            if (groups.includes(group) && group.state === 'active' && currentTabs(group.server).length)
              writer.schedule(() => groupSnapshot(group.server))
          })
      }
    } else if (group.state === 'active' && !group.detaching) {
      const active = currentTabs(group.server).find((tab) => tab.id === getActiveTabId())
      if (active) group.lastActiveTabId = active.id
      group.writer!.schedule(() => groupSnapshot(group.server))
    }
  }
  for (const server of servers) {
    if (findGroup(server)) continue
    const index = getTabs().findIndex((tab) => tabServer(tab) === server)
    const group = addGroup(server, crypto.randomUUID(), null, index)
    void attach(group, false, false)
  }
}

export function getGroupRefs(): PersistedGroupRef[] {
  const staged = new Set([...groupTransfers.values()].filter((transfer) => !transfer.source).map(({ group }) => group))
  return groups
    .filter((group) => !staged.has(group))
    .map(({ server, id, token, index }) => {
      const first = getTabs().findIndex((tab) => tabServer(tab) === server)
      return { server, id, controllerToken: token, index: first < 0 ? index : first }
    })
}

export async function flushActiveGroups(): Promise<void> {
  await Promise.all(groups.filter((group) => group.state === 'active').map((group) => group.writer!.flush()))
}

export async function restoreGroups(refs: PersistedGroupRef[]): Promise<void> {
  if (!platform.capabilities.remoteHosts) return
  await initGroupService()
  restoring = true
  try {
    const pending: { group: OwnedGroup; index: number }[] = []
    for (const ref of [...refs].sort((a, b) => a.index - b.index)) {
      if (!ref.server || !ref.id || findGroup(ref.server)) continue
      pending.push({ group: addGroup(ref.server, ref.id, ref.controllerToken, ref.index), index: ref.index })
    }
    const results = await Promise.all(
      pending.map(async ({ group, index }) => ({ group, index, snapshot: await attach(group, false, true, index) }))
    )
    for (const { group, index, snapshot } of results) {
      if (snapshot === null || !groups.includes(group)) continue
      try {
        hydrate(group, snapshot, index)
        if (group.state === 'active') group.writer!.schedule(() => groupSnapshot(group.server))
      } catch (error) {
        group.state = 'offline'
        console.warn('Workbench restore failed:', error)
      }
    }
  } finally {
    restoring = false
    syncGroups()
  }
}

export async function openWorkbench(server: string, id: string, takeover: boolean): Promise<void> {
  if (!platform.capabilities.remoteHosts) return
  await initGroupService()
  const existing = findGroup(server)
  if (existing) {
    if (existing.id !== id) {
      if (existing.state === 'active' || currentTabs(server).length)
        throw new Error(`This window already has a ${server} workbench`)
      abandon(existing)
    } else {
      if (takeover) await takeBackGroup(server)
      else if (existing.state === 'offline') await attach(existing, false, false, undefined, true)
      return
    }
  }
  await attach(addGroup(server, id, null), takeover, true, undefined, true)
}

/** Shows the tab this group last showed, else its first. */
export function focusGroup(server: string): void {
  const own = currentTabs(server)
  const last = findGroup(server)?.lastActiveTabId
  const id = own.some((tab) => tab.id === last) ? last : own[0]?.id
  if (id) setActiveTab(id)
}

export async function takeBackGroup(server: string): Promise<void> {
  const group = findGroup(server)
  if (group?.detaching) throw new Error('Workbench is already changing windows')
  if (!group) return
  await attach(group, true, true, undefined, true)
  // Taken back from its server tab: show the workbench, not whatever tab happened to be active.
  if (group.state === 'active') focusGroup(server)
}
export async function retryGroup(server: string): Promise<void> {
  const group = findGroup(server)
  if (group?.detaching) throw new Error('Workbench is already changing windows')
  if (group?.state === 'offline') await attach(group, false, false)
}
export async function removeGroupFromWindow(server: string): Promise<void> {
  const group = findGroup(server)
  if (!group || group.state === 'active') return
  if (group.detaching) throw new Error('Workbench is already changing windows')
  group.generation++
  await detachAttachment(group)
  await releaseViews(group)
  abandon(group)
  replaceGroupTabs(server, [])
}

/** Moves a group's tabs, in order, to insertion `slot` of the current tab list. */
export function moveGroup(server: string, slot: number): void {
  const own = currentTabs(server)
  const before = getTabs()
    .slice(0, slot)
    .filter((tab) => tabServer(tab) === server).length
  const active = own.findIndex((tab) => tab.id === getActiveTabId())
  replaceGroupTabs(server, own, slot - before, Math.max(active, 0))
}

export async function moveGroupToNewWindow(server: string): Promise<void> {
  const group = findGroup(server)
  if (!group || group.state !== 'active') return
  if (group.detaching) throw new Error('Workbench is already changing windows')
  await initGroupService()
  await requireNative().window.groupHandoff(getWorkbenchId(), server, group.id, 0)
}

export async function requestGroup(
  sourceWindow: string,
  server: string,
  workbenchId: string,
  index: number
): Promise<void> {
  const existing = findGroup(server)
  if (existing && (existing.detaching || existing.state === 'active' || currentTabs(server).length))
    throw new Error(`This window already has a ${server} workbench`)
  await initGroupService()
  await requireNative().window.groupHandoff(sourceWindow, server, workbenchId, index, getWorkbenchId())
}

async function exportGroup(handoff: GroupHandoff): Promise<void> {
  const group = findGroup(handoff.server)
  try {
    if (
      !group ||
      group.id !== handoff.workbenchId ||
      group.state !== 'active' ||
      group.detaching ||
      !group.attachmentId
    )
      throw new Error('Source workbench is not ready to move')
    let tabs = currentTabs(group.server)
    const generation = group.generation
    const attachmentId = group.attachmentId
    group.detaching = true
    groupTransfers.set(handoff.transferId, { group, source: true, tabs, previousActive: getActiveTabId() })
    setGroupTransferring(
      handoff.transferId,
      tabs.map((tab) => tab.id)
    )
    await group.writer!.flush()
    if (!groups.includes(group) || group.generation !== generation || group.state !== 'active')
      throw new Error('Source workbench lost control during the move')
    tabs = currentTabs(group.server)
    groupTransfers.get(handoff.transferId)!.tabs = tabs
    const modelStates = tabs.flatMap((tab) => {
      if (tab.kind !== 'text' || tab.gitRef) return []
      const state = modelCache.exportModelTransfer(tab.id)
      return state ? [state] : []
    })
    const activeTabId = tabs.some((tab) => tab.id === getActiveTabId()) ? getActiveTabId() : group.lastActiveTabId
    await requireNative().window.groupExported(handoff.transferId, attachmentId, tabs, activeTabId, modelStates)
  } catch (error) {
    await rejectGroup(handoff.transferId, error)
  }
}

async function stageGroup(handoff: GroupImport): Promise<void> {
  try {
    const stale = findGroup(handoff.server)
    if (stale && (stale.detaching || stale.state === 'active' || currentTabs(handoff.server).length))
      throw new Error(`This window already has a ${handoff.server} workbench`)
    if (!Array.isArray(handoff.tabs) || !handoff.tabs.every((tab) => isTab(tab) && tabServer(tab) === handoff.server))
      throw new Error('Invalid workbench transfer tabs')
    if (stale) {
      stale.detaching = true
      try {
        await detachAttachment(stale)
      } finally {
        stale.detaching = false
      }
      abandon(stale)
    }
    const group = addGroup(handoff.server, handoff.workbenchId, null, handoff.index)
    group.detaching = true
    group.lastActiveTabId = handoff.activeTabId
    groupTransfers.set(handoff.transferId, {
      group,
      source: false,
      tabs: handoff.tabs,
      previousActive: getActiveTabId()
    })
    setGroupTransferring(
      handoff.transferId,
      handoff.tabs.map((tab) => tab.id)
    )
    // Monaco accesses browser globals during import; SvelteKit also evaluates modules in Node at build time.
    const monaco = handoff.modelStates.length ? await import('monaco-editor') : null
    if (!groupTransfers.has(handoff.transferId)) return
    for (const state of handoff.modelStates) {
      const tab = handoff.tabs.find((candidate) => candidate.id === state.tabId)
      if (tab?.kind !== 'text' || state.folderPath !== tab.folderPath || state.filePath !== tab.filePath)
        throw new Error('Text model does not belong to the transferred workbench')
      if (monaco) modelCache.importModelTransfer(state, monaco)
    }
    for (const [index, tab] of handoff.tabs.entries()) {
      const staged: Tab =
        tab.kind === 'session' && tab.status !== 'exited' && tab.status !== 'failed'
          ? { ...tab, status: 'dormant' }
          : tab.kind === 'task' && tab.status !== 'exited' && tab.status !== 'failed'
            ? { ...tab, status: 'starting', attachOnly: true }
            : tab
      stageTransferredTab(staged, handoff.index + index)
    }
    await requireNative().window.groupStaged(handoff.transferId)
  } catch (error) {
    await rejectGroup(handoff.transferId, error)
  }
}

function commitGroup({ transferId }: GroupHandoff): void {
  const transfer = groupTransfers.get(transferId)
  if (!transfer?.source) return
  for (const tab of transfer.tabs) {
    cleanupRegistries(tab)
    removeTransferredTab(tab.id, false)
  }
  abandon(transfer.group)
  groupTransfers.delete(transferId)
  setGroupTransferring(transferId, null)
  persistWorkbench()
}

function finalizeGroup({ transferId, attached }: GroupFinalized): void {
  const transfer = groupTransfers.get(transferId)
  if (!transfer || transfer.source) return
  bindAttachment(transfer.group, attached)
  transfer.group.state = 'active'
  transfer.group.detaching = false
  for (const tab of transfer.tabs) finalizeTransferredTab(tab.id)
  groupTransfers.delete(transferId)
  setGroupTransferring(transferId, null)
  focusGroup(transfer.group.server)
  syncGroups()
  persistWorkbench()
  void refreshControl(transfer.group.server)
}

function abortGroup({ transferId }: GroupHandoff): void {
  const transfer = groupTransfers.get(transferId)
  if (!transfer) return
  groupTransfers.delete(transferId)
  setGroupTransferring(transferId, null)
  if (transfer.source) {
    transfer.group.detaching = false
    if (transfer.group.state === 'active') {
      syncGroups()
      void refreshControl(transfer.group.server)
    } else if (transfer.group.state === 'offline') void attach(transfer.group, false, false)
  } else {
    for (const tab of transfer.tabs) {
      cleanupRegistries(tab)
      removeTransferredTab(tab.id, false)
    }
    abandon(transfer.group)
    if (transfer.previousActive) setActiveTab(transfer.previousActive)
    persistWorkbench()
  }
}

async function rejectGroup(transferId: string, error: unknown): Promise<void> {
  await requireNative()
    .window.groupAbort(transferId, getErrorMessage(error))
    .catch((failure) => console.warn('Workbench handoff abort failed:', failure))
}

export async function closeGroupWorkbench(server: string): Promise<void> {
  const group = findGroup(server)
  if (!group) return
  if (group.detaching) throw new Error('Workbench is already changing windows')
  if (group.state === 'active') await group.writer!.flush()
  const previousState = group.state
  group.state = 'attaching'
  try {
    await backend.workbenches.close(group.id, server)
  } catch (error) {
    group.state = previousState
    throw error
  }
  group.writer?.stop()
  group.writer = null
  group.state = 'revoked'
  await releaseViews(group)
  abandon(group)
  replaceGroupTabs(server, [])
}
