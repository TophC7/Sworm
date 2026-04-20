// Typed frontend/backend bridge.
//
// Every Tauri invoke call goes through this module. Pages and
// components must NOT import invoke() directly.

import { invoke, Channel } from '@tauri-apps/api/core'
import type {
  AppInfo,
  CommitDetail,
  DiscoveredProject,
  DiffSource,
  FileDiff,
  EnvProbeResult,
  FileEntryStat,
  FilePasteCollision,
  GitSummary,
  GraphCommit,
  GeneralSettings,
  NixDetection,
  NixDiagnostic,
  NixEnvRecord,
  Project,
  ProviderStatus,
  PtyEvent,
  Session,
  SettingsPayload,
  StashEntry,
  ProviderConfig
} from '$lib/types/backend'

export const backend = {
  activityMap: {
    get(): Promise<DiscoveredProject[]> {
      return invoke<DiscoveredProject[]>('activity_map_get')
    },
    refresh(): Promise<DiscoveredProject[]> {
      return invoke<DiscoveredProject[]>('activity_map_refresh')
    }
  },

  app: {
    healthPing(): Promise<string> {
      return invoke<string>('health_ping')
    },
    getInfo(): Promise<AppInfo> {
      return invoke<AppInfo>('app_get_info')
    },
    dbSmokeTest(): Promise<string> {
      return invoke<string>('db_smoke_test')
    },
    keyringSmokeTest(): Promise<string> {
      return invoke<string>('keyring_smoke_test')
    },
    envProbe(): Promise<EnvProbeResult> {
      return invoke<EnvProbeResult>('env_probe')
    },
    /** Copy file paths to the system clipboard in file-manager format. */
    clipboardCopyFiles(paths: string[], op: 'copy' | 'cut'): Promise<void> {
      return invoke<void>('clipboard_copy_files', { paths, op })
    },
    /** Read file URIs from the system clipboard. Returns null if none. */
    clipboardReadFiles(): Promise<{ op: 'copy' | 'cut'; paths: string[] } | null> {
      return invoke<{ op: 'copy' | 'cut'; paths: string[] } | null>('clipboard_read_files')
    }
  },

  dnd: {
    saveDroppedBytes(bytes: Uint8Array, suggestedName: string): Promise<string> {
      return invoke<string>('dnd_save_dropped_bytes', { bytes: Array.from(bytes), suggestedName })
    }
  },

  projects: {
    selectDirectory(): Promise<string | null> {
      return invoke<string | null>('project_select_directory')
    },
    add(path: string): Promise<Project> {
      return invoke<Project>('project_add', { path })
    },
    openInTerminal(path: string): Promise<void> {
      return invoke<void>('project_open_in_terminal', { path })
    },
    list(): Promise<Project[]> {
      return invoke<Project[]>('project_list')
    },
    get(id: string): Promise<Project> {
      return invoke<Project>('project_get', { id })
    },
    remove(id: string): Promise<void> {
      return invoke<void>('project_remove', { id })
    }
  },

  providers: {
    list(): Promise<ProviderStatus[]> {
      return invoke<ProviderStatus[]>('provider_list')
    },
    refresh(): Promise<ProviderStatus[]> {
      return invoke<ProviderStatus[]>('provider_refresh')
    },
    listForProject(projectId: string): Promise<ProviderStatus[]> {
      return invoke<ProviderStatus[]>('provider_list_for_project', { projectId })
    }
  },

  sessions: {
    create(projectId: string, providerId: string, title: string): Promise<Session> {
      return invoke<Session>('session_create', { projectId, providerId, title })
    },
    list(projectId: string): Promise<Session[]> {
      return invoke<Session[]>('session_list', { projectId })
    },
    get(id: string): Promise<Session> {
      return invoke<Session>('session_get', { id })
    },
    createOutputChannel(onOutput: (data: number[]) => void): Channel<number[]> {
      const output = new Channel<number[]>()
      output.onmessage = onOutput
      return output
    },
    createEventChannel(onEvent: (event: PtyEvent) => void): Channel<PtyEvent> {
      const events = new Channel<PtyEvent>()
      events.onmessage = onEvent
      return events
    },
    startWithChannels(
      sessionId: string,
      cols: number,
      rows: number,
      output: Channel<number[]>,
      events: Channel<PtyEvent>
    ): Promise<void> {
      return invoke('session_start', { sessionId, cols, rows, output, events })
    },
    start(
      sessionId: string,
      cols: number,
      rows: number,
      onOutput: (data: number[]) => void,
      onEvent: (event: PtyEvent) => void
    ): Promise<void> {
      const output = backend.sessions.createOutputChannel(onOutput)
      const events = backend.sessions.createEventChannel(onEvent)
      return backend.sessions.startWithChannels(sessionId, cols, rows, output, events)
    },
    write(sessionId: string, data: Uint8Array): Promise<void> {
      return invoke('session_write', { sessionId, data: Array.from(data) })
    },
    resize(sessionId: string, cols: number, rows: number): Promise<void> {
      return invoke('session_resize', { sessionId, cols, rows })
    },
    stop(sessionId: string): Promise<void> {
      return invoke('session_stop', { sessionId })
    },
    reset(sessionId: string): Promise<void> {
      return invoke('session_reset', { sessionId })
    },
    remove(sessionId: string): Promise<void> {
      return invoke('session_remove', { sessionId })
    },
    archive(sessionId: string): Promise<void> {
      return invoke('session_archive', { sessionId })
    },
    unarchive(sessionId: string): Promise<void> {
      return invoke('session_unarchive', { sessionId })
    },
    listArchived(projectId: string): Promise<Session[]> {
      return invoke<Session[]>('session_list_archived', { projectId })
    },
    /**
     * Return whether the backend currently holds a live PTY for this
     * session. Distinct from `status` — survives the truth even after
     * a crash/restart that left a stale "running" row.
     */
    isAlive(sessionId: string): Promise<boolean> {
      return invoke<boolean>('session_is_alive', { sessionId })
    },
    /**
     * Fetch the persisted terminal transcript as base64-encoded bytes.
     * Used by the terminal mount path to replay history before any
     * live attach.
     */
    getTranscript(sessionId: string, limitBytes?: number): Promise<string> {
      return invoke<string>('session_transcript_get', {
        sessionId,
        limitBytes: limitBytes ?? null
      })
    }
  },

  workspace: {
    getState(projectId: string): Promise<string | null> {
      return invoke<string | null>('workspace_state_get', { projectId })
    },
    putState(projectId: string, stateJson: string): Promise<void> {
      return invoke<void>('workspace_state_put', { projectId, stateJson })
    },
    appStateGet(key: string): Promise<string | null> {
      return invoke<string | null>('app_state_get', { key })
    },
    appStatePut(key: string, valueJson: string): Promise<void> {
      return invoke<void>('app_state_put', { key, valueJson })
    }
  },

  git: {
    getSummary(path: string): Promise<GitSummary> {
      return invoke<GitSummary>('git_get_summary', { path })
    },
    getGraph(path: string, limit = 100): Promise<GraphCommit[]> {
      return invoke<GraphCommit[]>('git_get_graph', { path, limit })
    },
    getCommitDetail(path: string, hash: string): Promise<CommitDetail | null> {
      return invoke<CommitDetail | null>('git_get_commit_detail', { path, hash })
    },
    /**
     * Unified payload for the Monaco multi-file diff viewer.
     * Returns one `FileDiff` per changed file with both sides of
     * content attached. Source-agnostic (working / commit / stash).
     */
    getDiffFiles(path: string, source: DiffSource): Promise<FileDiff[]> {
      return invoke<FileDiff[]>('diff_get_files', { path, source })
    },

    // Write operations
    stageAll(path: string): Promise<void> {
      return invoke<void>('git_stage_all', { path })
    },
    stageFiles(path: string, files: string[]): Promise<void> {
      return invoke<void>('git_stage_files', { path, files })
    },
    unstageAll(path: string): Promise<void> {
      return invoke<void>('git_unstage_all', { path })
    },
    unstageFiles(path: string, files: string[]): Promise<void> {
      return invoke<void>('git_unstage_files', { path, files })
    },
    discardAll(path: string): Promise<void> {
      return invoke<void>('git_discard_all', { path })
    },
    discardFiles(path: string, files: string[]): Promise<void> {
      return invoke<void>('git_discard_files', { path, files })
    },
    getFullPatch(path: string): Promise<string | null> {
      return invoke<string | null>('git_get_full_patch', { path })
    },
    getPathPatch(path: string, files: string[], staged: boolean | null = null): Promise<string | null> {
      return invoke<string | null>('git_get_path_patch', { path, files, staged })
    },
    commit(path: string, message: string): Promise<string> {
      return invoke<string>('git_commit', { path, message })
    },
    undoLastCommit(path: string): Promise<string> {
      return invoke<string>('git_undo_last_commit', { path })
    },
    push(path: string): Promise<void> {
      return invoke<void>('git_push', { path })
    },
    pushForceWithLease(path: string): Promise<void> {
      return invoke<void>('git_push_force_with_lease', { path })
    },
    pull(path: string): Promise<void> {
      return invoke<void>('git_pull', { path })
    },
    fetch(path: string): Promise<void> {
      return invoke<void>('git_fetch', { path })
    },
    stashAll(path: string, message?: string): Promise<void> {
      return invoke<void>('git_stash_all', { path, message: message ?? null })
    },
    stashCount(path: string): Promise<number> {
      return invoke<number>('git_stash_count', { path })
    },
    stashList(path: string): Promise<StashEntry[]> {
      return invoke<StashEntry[]>('git_stash_list', { path })
    },
    stashPop(path: string, index: number): Promise<void> {
      return invoke<void>('git_stash_pop', { path, index })
    },
    stashDrop(path: string, index: number): Promise<void> {
      return invoke<void>('git_stash_drop', { path, index })
    },
    init(path: string): Promise<void> {
      return invoke<void>('git_init', { path })
    },
    cloneInPlace(path: string, url: string): Promise<void> {
      return invoke<void>('git_clone_in_place', { path, url })
    }
  },

  editor: {
    openFile(projectId: string, projectPath: string, filePath: string): Promise<void> {
      return invoke('editor_open_file', { projectId, projectPath, filePath })
    },
    openAtCommit(projectId: string, projectPath: string, commitHash: string, filePath: string): Promise<void> {
      return invoke('editor_open_at_commit', { projectId, projectPath, commitHash, filePath })
    },
    openAtStash(projectId: string, projectPath: string, stashIndex: number, filePath: string): Promise<void> {
      return invoke('editor_open_at_stash', { projectId, projectPath, stashIndex, filePath })
    },
    /** Return file content at a git revision (ref and path validated server-side). */
    showFile(projectPath: string, gitRef: string, filePath: string): Promise<string> {
      return invoke<string>('git_show_file', { projectPath, gitRef, filePath })
    }
  },

  files: {
    listAll(projectPath: string): Promise<string[]> {
      return invoke<string[]>('files_list_all', { projectPath })
    },
    read(projectPath: string, filePath: string): Promise<string> {
      return invoke<string>('file_read', { projectPath, filePath })
    },
    write(projectPath: string, filePath: string, content: string): Promise<void> {
      return invoke<void>('file_write', { projectPath, filePath, content })
    },
    createDir(projectPath: string, dirPath: string): Promise<void> {
      return invoke<void>('file_create_dir', { projectPath, dirPath })
    },
    rename(projectPath: string, oldPath: string, newPath: string): Promise<void> {
      return invoke<void>('file_rename', { projectPath, oldPath, newPath })
    },
    stat(projectPath: string, filePath: string): Promise<FileEntryStat | null> {
      return invoke<FileEntryStat | null>('file_stat', { projectPath, filePath })
    },
    delete(projectPath: string, filePath: string): Promise<void> {
      return invoke<void>('file_delete', { projectPath, filePath })
    },
    paste(
      projectPath: string,
      targetDir: string,
      op: 'copy' | 'cut',
      sources: string[],
      collisionPolicy: 'auto_rename' | 'replace' | 'skip' | 'rename' | 'error' = 'auto_rename',
      renameMap?: Record<string, string>
    ): Promise<string[]> {
      return invoke<string[]>('file_paste', {
        projectPath,
        targetDir,
        op,
        sources,
        collisionPolicy,
        renameMap: renameMap ?? null
      })
    },
    pasteCollisions(projectPath: string, targetDir: string, sources: string[]): Promise<FilePasteCollision[]> {
      return invoke<FilePasteCollision[]>('file_paste_collisions', { projectPath, targetDir, sources })
    }
  },

  nix: {
    detect(projectId: string): Promise<NixDetection> {
      return invoke<NixDetection>('nix_detect', { projectId })
    },
    select(projectId: string, nixFile: string): Promise<NixEnvRecord> {
      return invoke<NixEnvRecord>('nix_select', { projectId, nixFile })
    },
    evaluate(projectId: string): Promise<NixEnvRecord> {
      return invoke<NixEnvRecord>('nix_evaluate', { projectId })
    },
    clear(projectId: string): Promise<void> {
      return invoke<void>('nix_clear', { projectId })
    },
    status(projectId: string): Promise<NixEnvRecord | null> {
      return invoke<NixEnvRecord | null>('nix_status', { projectId })
    },
    format(content: string): Promise<string> {
      return invoke<string>('nix_format', { content })
    },
    lint(projectPath: string, filePath: string): Promise<NixDiagnostic[]> {
      return invoke<NixDiagnostic[]>('nix_lint', { projectPath, filePath })
    }
  },

  settings: {
    get(): Promise<SettingsPayload> {
      return invoke<SettingsPayload>('settings_get')
    },
    setGeneral(settings: GeneralSettings): Promise<GeneralSettings> {
      return invoke<GeneralSettings>('settings_set_general', { settings })
    },
    setProviderConfig(config: ProviderConfig): Promise<ProviderConfig> {
      return invoke<ProviderConfig>('settings_set_provider_config', { config })
    }
  }
}
