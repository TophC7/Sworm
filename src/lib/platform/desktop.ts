import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { getVersion } from '@tauri-apps/api/app'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager'
import { save } from '@tauri-apps/plugin-dialog'
import { openUrl, revealItemInDir } from '@tauri-apps/plugin-opener'
import { openDesktopPtyStream } from '$lib/api/transport.desktop'
import { resolveProjectFile } from '$lib/utils/paths'
import type {
  FocusTabPayload,
  OpenTarget,
  RemoteRunStatusEvent,
  RemoteSettings,
  RemoteStatus,
  RemoteStatusEvent,
  SettingsFileResult,
  ShortcutsFileResult,
  TabTransferExportPayload,
  TabTransferInitiateParams
} from '$lib/types/backend'
import type {
  GroupHandoff,
  GroupImport,
  GroupFinalized,
  GroupAborted,
  GroupSettled,
  NativePlatform,
  OsDropEvent,
  Platform,
  TransferAbortedEvent,
  TransferCommittedEvent,
  TransferFinalizedEvent,
  TransferImportEvent,
  TransferRequestEvent
} from './index'

function onEvent<T>(name: string, handler: (payload: T) => void): Promise<() => void> {
  return listen<T>(name, ({ payload }) => handler(payload))
}

function onWindowEvent<T>(name: string, handler: (payload: T) => void): Promise<() => void> {
  return getCurrentWindow().listen<T>(name, ({ payload }) => handler(payload))
}

const native: NativePlatform = {
  window: {
    create: (target) => invoke<string>('window_create', { target: target ?? null }),
    ready: (restoredTaskRuns) => invoke<void>('window_ready', { restoredTaskRuns }),
    close: () => invoke<void>('window_close'),
    onCloseRequested: (handler) => getCurrentWindow().onCloseRequested(handler),
    onOpenTarget: (handler) => onWindowEvent<OpenTarget>('open-target', handler),
    onFocusTab: (handler) => onEvent<FocusTabPayload>('focus-tab', handler),
    minimize: () => getCurrentWindow().minimize(),
    toggleMaximize: () => getCurrentWindow().toggleMaximize(),
    isMaximized: () => getCurrentWindow().isMaximized(),
    onResized: (handler) => getCurrentWindow().onResized(handler),
    setDecorations: (enabled) => getCurrentWindow().setDecorations(enabled),
    groupHandoff: (sourceWindow, server, workbenchId, index, targetWindow) =>
      invoke<void>('window_group_handoff', { sourceWindow, server, workbenchId, index, targetWindow }),
    groupExported: (transferId, attachmentId, tabs, activeTabId, modelStates) =>
      invoke<void>('window_group_exported', { transferId, attachmentId, tabs, activeTabId, modelStates }),
    groupStaged: (transferId) => invoke<void>('window_group_staged', { transferId }),
    groupAbort: (transferId, reason) => invoke<void>('window_group_abort', { transferId, reason }),
    onGroupRequest: (handler) => onWindowEvent<GroupHandoff>('group-transfer-request', handler),
    onGroupImport: (handler) => onWindowEvent<GroupImport>('group-transfer-import', handler),
    onGroupCommitted: (handler) => onWindowEvent<GroupHandoff>('group-transfer-committed', handler),
    onGroupFinalized: (handler) => onWindowEvent<GroupFinalized>('group-transfer-finalized', handler),
    onGroupAborted: (handler) => onWindowEvent<GroupAborted>('group-transfer-aborted', handler),
    onGroupSettled: (handler) => onEvent<GroupSettled>('group-transfer-settled', handler)
  },
  dialogs: {
    selectDirectory: () => invoke<string | null>('folder_select_directory'),
    saveAs: (options) => save(options)
  },
  files: {
    reveal: (path) => revealItemInDir(path),
    openInTerminal: (path) => invoke<void>('folder_open_in_terminal', { path }),
    clipboardCopyFiles: (paths, op) => invoke<void>('clipboard_copy_files', { paths, op }),
    clipboardReadFiles: () => invoke<{ op: 'copy' | 'cut'; paths: string[] } | null>('clipboard_read_files'),
    claimFile: (filePath, tabId, reveal = null) => invoke('window_claim_file', { filePath, tabId, reveal }),
    releaseFile: (filePath) => invoke<void>('window_release_file', { filePath })
  },
  settings: {
    openGlobalFile: () => invoke<SettingsFileResult>('settings_open_global_file')
  },
  shortcuts: { openGlobalFile: () => invoke<ShortcutsFileResult>('shortcuts_open_global_file') },
  deepLinks: {
    take: () => invoke<string[]>('deep_link_take'),
    onOpen: (handler) => onEvent<void>('deep-link-open', handler)
  },
  osDrop: {
    onEvent: (handler) => getCurrentWebview().onDragDropEvent(({ payload }) => handler(payload as OsDropEvent)),
    saveDroppedBytes: (bytes, suggestedName) =>
      invoke<string>('dnd_save_dropped_bytes', {
        bytes: Array.from(bytes),
        suggestedName
      })
  },
  transfers: {
    initiate: (params: TabTransferInitiateParams) => invoke<string>('window_transfer_initiate', { params }),
    sourceExported: (payload: TabTransferExportPayload) => invoke<void>('window_transfer_source_exported', { payload }),
    targetStaged: (transferId) => invoke<void>('window_transfer_target_staged', { transferId }),
    abort: (transferId, reason) => invoke<void>('window_transfer_abort', { transferId, reason }),
    onRequest: (handler) => onWindowEvent<TransferRequestEvent>('tab-transfer-request', handler),
    onImport: (handler) => onWindowEvent<TransferImportEvent>('tab-transfer-import', handler),
    onCommitted: (handler) => onWindowEvent<TransferCommittedEvent>('tab-transfer-committed', handler),
    onFinalized: (handler) => onWindowEvent<TransferFinalizedEvent>('tab-transfer-finalized', handler),
    onAborted: (handler) => onWindowEvent<TransferAbortedEvent>('tab-transfer-aborted', handler),
    pause: (runId) => invoke<number>('pty_pause', { runId }),
    attach: (runId, transferId, sinks) => openDesktopPtyStream<number>('pty_attach', { runId, transferId }, sinks)
  },
  remotes: {
    pair: (link, name) => invoke<RemoteSettings>('pair_remote', { link, name }),
    repair: (link, name) => invoke<RemoteSettings>('repair_remote', { link, name }),
    clientFingerprint: () => invoke<string>('remote_client_fingerprint'),
    status: (server) => invoke<RemoteStatus>('remote_status', { server }),
    rename: (server, name) => invoke<void>('rename_remote', { server, name }),
    remove: (server) => invoke<void>('remove_remote', { server }),
    onStatus: (handler) => onEvent<RemoteStatusEvent>('remote-status', handler),
    onRunStatus: (handler) => onEvent<RemoteRunStatusEvent>('remote-run-status', handler),
    releaseRuns: (runIds) => invoke<void>('remote_runs_release', { runIds })
  },
  zoom: { setZoom: (level) => getCurrentWebview().setZoom(level) }
}

export const desktopPlatform: Platform = {
  workbench: {
    get id() {
      return getCurrentWindow().label
    }
  },
  app: { version: getVersion },
  clipboard: { readText, writeText },
  links: { openExternal: openUrl },
  focus: { onChanged: (handler) => getCurrentWindow().onFocusChanged(({ payload }) => handler(payload)) },
  assets: { url: (folderPath, filePath) => convertFileSrc(resolveProjectFile(folderPath, filePath)) },
  capabilities: {
    revealInFileManager: true,
    openInTerminal: true,
    tabTransfer: true,
    deepLinks: true,
    osDragDrop: true,
    nativeWindowControls: true,
    zoom: true,
    saveAsDialog: true,
    nativeFileClipboard: true,
    nativeDirectoryPicker: true,
    externalFileOpen: true,
    fileClaims: true,
    remoteHosts: true,
    durableWorkbenches: false
  },
  native
}
