import type { PtySinks, StreamHandle, Unsubscribe } from '$lib/api/transport'
import type { TextRevealTarget } from '$lib/features/workbench/surfaces/text/service.svelte'
import type { Tab } from '$lib/features/workbench/model'
import type {
  ClaimFileResult,
  FocusTabPayload,
  OpenTarget,
  RemoteRunStatusEvent,
  RemoteSettings,
  RemoteStatus,
  RemoteStatusEvent,
  SettingsFileResult,
  ShortcutsFileResult,
  TabTransferAbortedPayload,
  TabTransferExportPayload,
  TabTransferInitiateParams,
  TextModelTransferState,
  WorkbenchAttached
} from '$lib/types/backend'

export type { Unsubscribe } from '$lib/api/transport'

export interface PlatformCapabilities {
  revealInFileManager: boolean
  openInTerminal: boolean
  tabTransfer: boolean
  deepLinks: boolean
  osDragDrop: boolean
  nativeWindowControls: boolean
  zoom: boolean
  saveAsDialog: boolean
  nativeFileClipboard: boolean
  nativeDirectoryPicker: boolean
  externalFileOpen: boolean
  fileClaims: boolean
  remoteHosts: boolean
  /** Server-owned workbenches the user can switch between and close (web). */
  durableWorkbenches: boolean
}

export interface TransferRequestEvent {
  transferId: string
  tabId: string
}
export interface TransferImportEvent {
  transferId: string
  exportPayload: TabTransferExportPayload
  targetIndex: number
}
export interface TransferCommittedEvent {
  transferId: string
  tabId: string
}
export interface TransferFinalizedEvent {
  transferId: string
}
export type TransferAbortedEvent = TabTransferAbortedPayload

/** Router-coordinated local move; server control remains on the desktop connection. */
export interface GroupHandoff {
  transferId: string
  sourceWindow: string
  server: string
  workbenchId: string
  targetWindow: string
  index: number
}

export interface GroupImport extends GroupHandoff {
  tabs: Tab[]
  activeTabId: string | null
  modelStates: TextModelTransferState[]
}

export interface GroupFinalized extends GroupHandoff {
  attached: Extract<WorkbenchAttached, { kind: 'ready' }>
}

export interface GroupAborted extends GroupHandoff {
  reason: string
}

export type GroupSettled = GroupHandoff &
  (
    | { committed: true; attached: Extract<WorkbenchAttached, { kind: 'ready' }>; reason?: string }
    | { committed: false; reason?: string }
  )

export type OsDropEvent =
  | { type: 'enter' | 'drop'; paths: string[]; position: { x: number; y: number } }
  | { type: 'over'; position: { x: number; y: number } }
  | { type: 'leave' }

export interface NativePlatform {
  window: {
    create(target?: OpenTarget): Promise<string>
    ready(restoredTaskRuns: string[]): Promise<void>
    close(): Promise<void>
    onCloseRequested(handler: (event: { preventDefault(): void }) => void): Promise<Unsubscribe>
    onOpenTarget(handler: (target: OpenTarget) => void): Promise<Unsubscribe>
    onFocusTab(handler: (event: FocusTabPayload) => void): Promise<Unsubscribe>
    minimize(): Promise<void>
    toggleMaximize(): Promise<void>
    isMaximized(): Promise<boolean>
    onResized(handler: () => void): Promise<Unsubscribe>
    setDecorations(enabled: boolean): Promise<void>
    groupHandoff(
      sourceWindow: string,
      server: string,
      workbenchId: string,
      index: number,
      targetWindow?: string
    ): Promise<void>
    groupExported(
      transferId: string,
      attachmentId: string,
      tabs: GroupImport['tabs'],
      activeTabId: string | null,
      modelStates: GroupImport['modelStates']
    ): Promise<void>
    groupStaged(transferId: string): Promise<void>
    groupAbort(transferId: string, reason: string): Promise<void>
    onGroupRequest(handler: (handoff: GroupHandoff) => void): Promise<Unsubscribe>
    onGroupImport(handler: (handoff: GroupImport) => void): Promise<Unsubscribe>
    onGroupCommitted(handler: (handoff: GroupHandoff) => void): Promise<Unsubscribe>
    onGroupFinalized(handler: (handoff: GroupFinalized) => void): Promise<Unsubscribe>
    onGroupAborted(handler: (handoff: GroupAborted) => void): Promise<Unsubscribe>
    onGroupSettled(handler: (handoff: GroupSettled) => void): Promise<Unsubscribe>
  }
  dialogs: {
    selectDirectory(): Promise<string | null>
    saveAs(options: {
      title?: string
      defaultPath?: string
      filters?: { name: string; extensions: string[] }[]
    }): Promise<string | null>
  }
  files: {
    reveal(path: string): Promise<void>
    openInTerminal(path: string): Promise<void>
    clipboardCopyFiles(paths: string[], op: 'copy' | 'cut'): Promise<void>
    clipboardReadFiles(): Promise<{ op: 'copy' | 'cut'; paths: string[] } | null>
    claimFile(filePath: string, tabId: string, reveal?: TextRevealTarget | null): Promise<ClaimFileResult>
    releaseFile(filePath: string): Promise<void>
  }
  settings: {
    openGlobalFile(): Promise<SettingsFileResult>
  }
  shortcuts: { openGlobalFile(): Promise<ShortcutsFileResult> }
  deepLinks: {
    take(): Promise<string[]>
    onOpen(handler: () => void): Promise<Unsubscribe>
  }
  osDrop: {
    onEvent(handler: (event: OsDropEvent) => void): Promise<Unsubscribe>
    saveDroppedBytes(bytes: Uint8Array, suggestedName: string): Promise<string>
  }
  transfers: {
    initiate(params: TabTransferInitiateParams): Promise<string>
    sourceExported(payload: TabTransferExportPayload): Promise<void>
    targetStaged(transferId: string): Promise<void>
    abort(transferId: string, reason: string): Promise<void>
    onRequest(handler: (event: TransferRequestEvent) => void): Promise<Unsubscribe>
    onImport(handler: (event: TransferImportEvent) => void): Promise<Unsubscribe>
    onCommitted(handler: (event: TransferCommittedEvent) => void): Promise<Unsubscribe>
    onFinalized(handler: (event: TransferFinalizedEvent) => void): Promise<Unsubscribe>
    onAborted(handler: (event: TransferAbortedEvent) => void): Promise<Unsubscribe>
    pause(runId: string): Promise<number>
    attach(runId: string, transferId: string, sinks: PtySinks): StreamHandle<number>
  }
  remotes: {
    pair(link: string, name: string): Promise<RemoteSettings>
    repair(link: string, name: string): Promise<RemoteSettings>
    clientFingerprint(): Promise<string>
    status(server: string): Promise<RemoteStatus>
    rename(server: string, name: string): Promise<void>
    remove(server: string): Promise<void>
    onStatus(handler: (event: RemoteStatusEvent) => void): Promise<Unsubscribe>
    onRunStatus(handler: (event: RemoteRunStatusEvent) => void): Promise<Unsubscribe>
    releaseRuns(runIds: string[]): Promise<void>
  }
  zoom: { setZoom(level: number): Promise<void> }
}

export interface AssetHandle {
  url: string
  dispose(): void
}

export interface Platform {
  readonly workbench: { readonly id: string; closeCurrent?(): Promise<void>; takeOver?(id: string): Promise<void> }
  app: { version(): Promise<string> }
  clipboard: { readText(): Promise<string>; writeText(text: string): Promise<void> }
  links: { openExternal(url: string): Promise<void> }
  focus: { onChanged(handler: (focused: boolean) => void): Promise<Unsubscribe> }
  // Native assets have no allocated URL resource; web assets do.
  assets:
    | { url(folderPath: string, filePath: string): string }
    | { load(folderPath: string, filePath: string, signal: AbortSignal): Promise<AssetHandle> }
  capabilities: PlatformCapabilities
  native: NativePlatform | null
}

let installed: Platform | undefined

/** Imports are safe before route setup; actual access needs explicit installation. */
export const platform: Platform = new Proxy({} as Platform, {
  get(_target, property) {
    if (!installed) throw new Error('Platform is not installed')
    return Reflect.get(installed, property)
  }
})

export function installPlatform(value: Platform): void {
  installed = value
}

export function requireNative(): NativePlatform {
  const native = platform.native
  if (!native) throw new Error('Native platform unavailable')
  return native
}
