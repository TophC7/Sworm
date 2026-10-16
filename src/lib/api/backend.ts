// Typed host API. Desktop and future web transports provide the same calls.

import { getHostTransport, type PtySinks, type StreamHandle, type Unsubscribe } from './transport'
import type {
  AppRuntimeInfo,
  BranchSummary,
  BuiltinCatalog,
  CommitDetail,
  ConfigSchemaEntry,
  DiffFileContent,
  DiffSource,
  DiscoveredProject,
  EffectiveSettingsPayload,
  ExplorerDirEntry,
  ExplorerPathList,
  FileContent,
  FileStat,
  FileReadProgress,
  FileDiff,
  FilePasteMapping,
  FilePasteCollision,
  FilePathChangedPayload,
  FilesChangedEvent,
  FormattingSettings,
  NixSettings,
  TerminalSettings,
  WindowSettings,
  GitBrief,
  GitBranchOp,
  GitCommitOptions,
  GitConflictsOp,
  GitOperationKind,
  GitPathsOp,
  GitChangedEvent,
  GitQuickDiffData,
  GitRemoteOp,
  GitSequencerAction,
  GitStashOp,
  GitSummary,
  GitWorktreeCheckout,
  GitWorktreeList,
  GitWorktreeOp,
  GitWorktreeResult,
  GraphCommit,
  Issue,
  IssueComment,
  IssueCommentCreateInput,
  IssueCreateInput,
  IssueDetail,
  IssueEpic,
  IssueEpicCreateInput,
  IssueEpicUpdateInput,
  IssueListFilters,
  IssueUpdateInput,
  LspEvent,
  LspServerConfig,
  LspServerSettingsEntry,
  NixDetection,
  NixDiagnostic,
  NixEnvRecord,
  FolderEntry,
  FolderInfo,
  PathRoot,
  ProviderConfig,
  ProviderStatus,
  SessionSpec,
  SessionStartInfo,
  SettingsChangedEvent,
  SettingsFileResult,
  SettingsPayload,
  ShortcutsFilePayload,
  StashEntry,
  TaskDefinition,
  RecentFolder,
  AttachMode,
  WorkbenchAttached,
  WorkbenchInfo
} from '$lib/types/backend'

// Keep the command names and argument shapes at the host facade boundary.
function invoke<T>(method: string, params: object = {}): Promise<T> {
  return getHostTransport().call<T>(method, params)
}

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
    runtimeInfo(): Promise<AppRuntimeInfo> {
      return invoke<AppRuntimeInfo>('app_runtime_info')
    },
    stateGet(key: string): Promise<string | null> {
      return invoke<string | null>('app_state_get', { key })
    },
    statePut(key: string, valueJson: string): Promise<void> {
      return invoke<void>('app_state_put', { key, valueJson })
    },
    stateDelete(key: string): Promise<void> {
      return invoke<void>('app_state_delete', { key })
    }
  },

  /** Server-owned workbench registry; attaching to an unknown id creates it. */
  workbenches: {
    list(server?: string): Promise<WorkbenchInfo[]> {
      return invoke<WorkbenchInfo[]>('workbench_list', { server })
    },
    /** Stops the workbench's sessions/tasks and deletes its saved state. */
    close(id: string, server?: string): Promise<void> {
      return invoke<void>('workbench_close', { id, server })
    },
    attach(server: string, id: string, mode: AttachMode, attachmentId: string): Promise<WorkbenchAttached | null> {
      return invoke<WorkbenchAttached | null>('workbench_attach', { server, id, mode, attachmentId })
    },
    detach(server: string, id: string, attachmentId: string): Promise<void> {
      return invoke<void>('workbench_detach', { server, id, attachmentId })
    },
    save(server: string, id: string, snapshot: string): Promise<void> {
      return invoke<void>('workbench_save', { server, id, snapshot })
    },
    onChanged(handler: (event: { server: string | null }) => void): Promise<Unsubscribe> {
      return getHostTransport().subscribe('workbenches-changed', handler)
    }
  },

  folders: {
    recentList(): Promise<RecentFolder[]> {
      return invoke<RecentFolder[]>('recent_folders_list')
    },
    recentTouch(path: string): Promise<RecentFolder[]> {
      return invoke<RecentFolder[]>('recent_folders_touch', { path })
    },
    recentRemove(paths: string[]): Promise<RecentFolder[]> {
      return invoke<RecentFolder[]>('recent_folders_remove', { paths })
    },
    onRecentFoldersChanged(handler: (folders: RecentFolder[]) => void): Promise<Unsubscribe> {
      return getHostTransport().subscribe('recent-folders-changed', handler)
    },
    claim(folderPath: string): Promise<void> {
      return invoke<void>('folder_claim', { folderPath })
    },
    /** Canonicalize a folder path; rejects missing paths and non-directories. */
    resolve(path: string): Promise<FolderInfo> {
      return invoke<FolderInfo>('folder_resolve', { path })
    },
    listEntries(path: string, showHidden: boolean): Promise<FolderEntry[]> {
      return invoke<FolderEntry[]>('folder_list_entries', { path, showHidden })
    },
    /** Where the browser's path bar starts for a folder; remote roots come back as `sworm://` paths. */
    pathRoot(path: string): Promise<PathRoot> {
      return invoke<PathRoot>('folder_path_root', { path })
    },
    /** Canonical home folder of `server`, or of this host when omitted. */
    async home(server?: string): Promise<string> {
      const home = await invoke<string>('folder_home', { folderPath: server ? `sworm://${server}/` : null })
      return server ? `sworm://${server}${home}` : home
    },
    /** Canonical process working folder of `server`, or of this host when omitted. */
    async workingDirectory(server?: string): Promise<string> {
      const path = await invoke<string>('folder_working_directory', {
        folderPath: server ? `sworm://${server}/` : null
      })
      return server ? `sworm://${server}${path}` : path
    },
    /** Drop backend resources scoped to a folder that no longer has any open tab. */
    release(folderPath: string): Promise<void> {
      return invoke<void>('folder_release', { folderPath })
    }
  },

  providers: {
    list(): Promise<ProviderStatus[]> {
      return invoke<ProviderStatus[]>('provider_list')
    },
    listForFolder(folderPath: string): Promise<ProviderStatus[]> {
      return invoke<ProviderStatus[]>('provider_list_for_folder', { folderPath })
    }
  },

  sessions: {
    /** Reattach a matching retained run, or start a new run using provider resume policy. */
    start(spec: SessionSpec, cols: number, rows: number, sinks: PtySinks): StreamHandle<SessionStartInfo> {
      return getHostTransport().openStream({ method: 'session_start', params: { ...spec, cols, rows } }, sinks)
    },
    write(runId: string, data: Uint8Array): Promise<void> {
      return invoke<void>('session_write', {
        runId,
        data: Array.from(data)
      })
    },
    resize(runId: string, cols: number, rows: number): Promise<void> {
      return invoke<void>('session_resize', { runId, cols, rows })
    },
    stop(runId: string): Promise<void> {
      return invoke<void>('session_stop', { runId })
    },
    ompResolveUri(uri: string, cwd?: string | null): Promise<{ path: string; is_dir: boolean }> {
      return invoke<{ path: string; is_dir: boolean }>('omp_resolve_uri', { uri, cwd: cwd ?? null })
    }
  },

  git: {
    listWorktrees(path: string): Promise<GitWorktreeList> {
      return invoke<GitWorktreeList>('git_worktree_list', { path })
    },
    defaultWorktreePath(path: string, checkout: GitWorktreeCheckout, root: string | null = null): Promise<string> {
      return invoke<string>('git_worktree_default_path', { path, checkout, root })
    },
    worktree(path: string, op: GitWorktreeOp): Promise<GitWorktreeResult> {
      return invoke<GitWorktreeResult>('git_worktree', { path, op })
    },
    getSummary(path: string): Promise<GitSummary> {
      return invoke<GitSummary>('git_get_summary', { path })
    },
    /** Branch, changed-path count and ahead/behind from one `git status`; for Home cards. */
    getBrief(path: string): Promise<GitBrief> {
      return invoke<GitBrief>('git_get_brief', { path })
    },
    /** Arm or repair the folder's git watcher; idempotent and silent on non-repos. */
    watch(projectPath: string): Promise<void> {
      return invoke<void>('git_watch', { projectPath })
    },
    onChanged(handler: (event: GitChangedEvent) => void): Promise<Unsubscribe> {
      return getHostTransport().subscribe('git-changed', handler)
    },
    getGraph(path: string, limit = 100): Promise<GraphCommit[]> {
      return invoke<GraphCommit[]>('git_get_graph', { path, limit })
    },
    getCommitDetail(path: string, hash: string): Promise<CommitDetail | null> {
      return invoke<CommitDetail | null>('git_get_commit_detail', {
        path,
        hash
      })
    },
    /** Return file content at a git revision (ref and path validated server-side). */
    showFile(projectPath: string, gitRef: string, filePath: string): Promise<string> {
      return invoke<string>('git_show_file', { projectPath, gitRef, filePath })
    },
    /**
     * Load one file's contents for a working-tree, commit or stash diff.
     */
    getDiffFile(
      path: string,
      source: DiffSource,
      filePath: string,
      oldPath: string | null,
      status: FileDiff['status']
    ): Promise<DiffFileContent> {
      return invoke<DiffFileContent>('diff_get_file', { path, source, filePath, oldPath, status })
    },
    /**
     * Cheap working-tree diff index; file list + metadata only,
     * no content. Pair with [`getDiffFile`] to load each
     * file lazily so a 200-file working tree doesn't ship a
     * multi-megabyte payload before the user has expanded any row.
     */
    getWorkingDiffIndex(path: string, staged: boolean): Promise<FileDiff[]> {
      return invoke<FileDiff[]>('diff_get_working_index', { path, staged })
    },

    // Write operations
    paths(path: string, op: GitPathsOp): Promise<void> {
      return invoke<void>('git_paths', { path, op })
    },
    getFullPatch(path: string): Promise<string | null> {
      return invoke<string | null>('git_get_full_patch', { path })
    },
    getPathPatch(path: string, files: string[], staged: boolean | null = null): Promise<string | null> {
      return invoke<string | null>('git_get_path_patch', {
        path,
        files,
        staged
      })
    },
    getQuickDiffData(projectPath: string, filePath: string): Promise<GitQuickDiffData> {
      return invoke<GitQuickDiffData>('git_get_quick_diff_data', {
        projectPath,
        filePath
      })
    },
    stageFileContent(projectPath: string, filePath: string, content: string | null): Promise<void> {
      return invoke<void>('git_stage_file_content', {
        projectPath,
        filePath,
        content
      })
    },
    commit(path: string, message: string, options: Partial<GitCommitOptions> = {}): Promise<string> {
      return invoke<string>('git_commit', {
        path,
        message,
        options: { amend: false, all: false, signOff: false, noVerify: false, ...options }
      })
    },
    undoLastCommit(path: string): Promise<string> {
      return invoke<string>('git_undo_last_commit', { path })
    },
    remote(path: string, op: GitRemoteOp): Promise<void> {
      return invoke<void>('git_remote', { path, op })
    },
    stash(path: string, op: GitStashOp): Promise<void> {
      return invoke<void>('git_stash', { path, op })
    },
    stashCount(path: string): Promise<number> {
      return invoke<number>('git_stash_count', { path })
    },
    stashList(path: string): Promise<StashEntry[]> {
      return invoke<StashEntry[]>('git_stash_list', { path })
    },
    init(path: string): Promise<void> {
      return invoke<void>('git_init', { path })
    },
    cloneInPlace(path: string, url: string): Promise<void> {
      return invoke<void>('git_clone_in_place', { path, url })
    },

    listBranches(path: string): Promise<BranchSummary[]> {
      return invoke<BranchSummary[]>('git_list_branches', { path })
    },
    getBranchCommits(path: string, branch: string, limit = 5): Promise<GraphCommit[]> {
      return invoke<GraphCommit[]>('git_get_branch_commits', { path, branch, limit })
    },
    diffBranchAgainstHead(path: string, branch: string): Promise<FileDiff[]> {
      return invoke<FileDiff[]>('git_diff_branch_against_head', { path, branch })
    },
    branch(path: string, op: GitBranchOp): Promise<void> {
      return invoke<void>('git_branch', { path, op })
    },
    sequencer(path: string, operation: GitOperationKind, action: GitSequencerAction): Promise<void> {
      return invoke<void>('git_sequencer', { path, operation, action })
    },
    conflicts(path: string, op: GitConflictsOp): Promise<void> {
      return invoke<void>('git_conflicts', { path, op })
    }
  },

  files: {
    stat(folderPath: string, filePath: string): Promise<FileStat> {
      return invoke<FileStat>('file_stat', { projectPath: folderPath, filePath })
    },
    /** `version` and `size` are the approved stat; the open re-checks that identity once. */
    readStream(
      requestId: string,
      folderPath: string,
      filePath: string,
      version: string,
      size: number
    ): Promise<FileContent> {
      return invoke<FileContent>('file_read_stream', { requestId, projectPath: folderPath, filePath, version, size })
    },
    cancelReadStream(requestId: string): Promise<void> {
      return invoke<void>('file_read_stream_cancel', { requestId })
    },
    onReadProgress(handler: (event: FileReadProgress) => void): Promise<Unsubscribe> {
      return getHostTransport().subscribe('file-read-progress', handler)
    },
    /** One directory as the explorer renders it. `dirPath` '' is the project root. */
    readDir(projectPath: string, dirPath: string, showHidden: boolean): Promise<ExplorerDirEntry[]> {
      return invoke<ExplorerDirEntry[]>('files_read_dir', { projectPath, dirPath, showHidden })
    },
    /** Flat searchable path list for Quick Open and the sidebar filter. */
    listPaths(projectPath: string, showHidden: boolean): Promise<ExplorerPathList> {
      return invoke<ExplorerPathList>('files_list_paths', { projectPath, showHidden })
    },
    /** Watch exactly the directories currently rendered. */
    watchDirs(projectPath: string, dirs: string[]): Promise<void> {
      return invoke<void>('files_watch_dirs', { projectPath, dirs })
    },
    onChanged(handler: (event: FilesChangedEvent) => void): Promise<Unsubscribe> {
      return getHostTransport().subscribe('files-changed', handler)
    },
    onPathChanged(handler: (event: FilePathChangedPayload) => void): Promise<Unsubscribe> {
      return getHostTransport().subscribe('file-path-changed', handler)
    },
    onDeleted(handler: (event: { filePath: string }) => void): Promise<Unsubscribe> {
      return getHostTransport().subscribe('file-deleted', handler)
    },
    /**
     * File text plus the version of the bytes read, for conflict-checked writes.
     * Files over 16 MiB reject with `{ kind: 'tooLarge', size, limit }`; those stream.
     */
    read(projectPath: string, filePath: string): Promise<FileContent> {
      return invoke<FileContent>('file_read', { projectPath, filePath })
    },
    /**
     * `expectedVersion` is the version the caller last read; a mismatch rejects
     * the write with a `conflict` error carrying the on-disk version. `null`
     * overwrites whatever is there. Resolves with the written content's version.
     */
    write(
      projectPath: string,
      filePath: string,
      content: string,
      expectedVersion: string | null = null
    ): Promise<string> {
      return invoke<string>('file_write', { projectPath, filePath, content, expectedVersion })
    },
    createDir(projectPath: string, dirPath: string): Promise<void> {
      return invoke<void>('file_create_dir', { projectPath, dirPath })
    },
    rename(projectPath: string, oldPath: string, newPath: string): Promise<void> {
      return invoke<void>('file_rename', { projectPath, oldPath, newPath })
    },
    delete(projectPath: string, filePath: string, permanent = false): Promise<void> {
      return invoke<void>('file_delete', { projectPath, filePath, permanent })
    },
    paste(
      projectPath: string,
      targetDir: string,
      op: 'copy' | 'cut',
      sources: string[],
      collisionPolicy: 'auto_rename' | 'replace' | 'skip' | 'rename' | 'error' = 'auto_rename',
      renameMap?: Record<string, string>,
      permanent = false
    ): Promise<FilePasteMapping[]> {
      return invoke<FilePasteMapping[]>('file_paste', {
        projectPath,
        targetDir,
        op,
        sources,
        collisionPolicy,
        renameMap: renameMap ?? null,
        permanent
      })
    },
    pasteCollisions(projectPath: string, targetDir: string, sources: string[]): Promise<FilePasteCollision[]> {
      return invoke<FilePasteCollision[]>('file_paste_collisions', {
        projectPath,
        targetDir,
        sources
      })
    }
  },

  nix: {
    onChanged(handler: (event: { folderPath: string }) => void): Promise<Unsubscribe> {
      return getHostTransport().subscribe('nix-changed', handler)
    },
    detect(folderPath: string): Promise<NixDetection> {
      return invoke<NixDetection>('nix_detect', { folderPath })
    },
    select(folderPath: string, nixFile: string): Promise<NixEnvRecord> {
      return invoke<NixEnvRecord>('nix_select', { folderPath, nixFile })
    },
    evaluate(folderPath: string): Promise<NixEnvRecord> {
      return invoke<NixEnvRecord>('nix_evaluate', { folderPath })
    },
    clear(folderPath: string): Promise<void> {
      return invoke<void>('nix_clear', { folderPath })
    },
    lint(folderPath: string, filePath: string): Promise<NixDiagnostic[]> {
      return invoke<NixDiagnostic[]>('nix_lint', { folderPath, filePath })
    }
  },

  settings: {
    // The global layer of the host that runs `folderPath` — the exact layer
    // the setters below write. Omit it for this desktop.
    get(folderPath?: string): Promise<SettingsPayload> {
      return invoke<SettingsPayload>('settings_get', { folderPath: folderPath ?? null })
    },
    getEffective(folderPath?: string): Promise<EffectiveSettingsPayload> {
      return invoke<EffectiveSettingsPayload>('settings_get_effective', { folderPath: folderPath ?? null })
    },
    openFolderFile(folderPath: string): Promise<SettingsFileResult> {
      return invoke<SettingsFileResult>('settings_open_folder_file', { folderPath })
    },
    onChanged(handler: (event: SettingsChangedEvent) => void): Promise<Unsubscribe> {
      return getHostTransport().subscribe('settings-changed', handler)
    },
    setWindow(settings: WindowSettings): Promise<WindowSettings> {
      return invoke<WindowSettings>('settings_set_window', { settings })
    },
    setTerminal(settings: TerminalSettings): Promise<TerminalSettings> {
      return invoke<TerminalSettings>('settings_set_terminal', { settings })
    },
    setNix(settings: NixSettings, folderPath?: string): Promise<NixSettings> {
      return invoke<NixSettings>('settings_set_nix', { settings, folderPath: folderPath ?? null })
    },
    setFormatting(formatting: FormattingSettings, folderPath?: string): Promise<FormattingSettings> {
      return invoke<FormattingSettings>('settings_set_formatting', {
        formatting,
        folderPath: folderPath ?? null
      })
    },
    setProviderConfig(config: ProviderConfig, folderPath?: string): Promise<ProviderConfig> {
      return invoke<ProviderConfig>('settings_set_provider_config', { config, folderPath: folderPath ?? null })
    }
  },

  shortcuts: {
    getGlobal(): Promise<ShortcutsFilePayload> {
      return invoke<ShortcutsFilePayload>('shortcuts_get_global')
    },
    setGlobal(value: unknown): Promise<ShortcutsFilePayload> {
      return invoke<ShortcutsFilePayload>('shortcuts_set_global', { value })
    }
  },

  builtins: {
    getCatalog(): Promise<BuiltinCatalog> {
      return invoke<BuiltinCatalog>('builtins_get_catalog')
    }
  },

  configSchemas: {
    list(): Promise<ConfigSchemaEntry[]> {
      return invoke<ConfigSchemaEntry[]>('config_schemas_list')
    }
  },

  issues: {
    onChanged(handler: (event: { folderPath: string }) => void): Promise<Unsubscribe> {
      return getHostTransport().subscribe('issues-changed', handler)
    },
    list(folderPath: string, filters: IssueListFilters = {}): Promise<Issue[]> {
      return invoke<Issue[]>('issues_list', { folderPath, filters })
    },
    get(folderPath: string, issueId: string): Promise<IssueDetail> {
      return invoke<IssueDetail>('issues_get', { folderPath, issueId })
    },
    create(folderPath: string, input: IssueCreateInput): Promise<Issue> {
      return invoke<Issue>('issues_create', { folderPath, input })
    },
    update(folderPath: string, issueId: string, patch: IssueUpdateInput): Promise<Issue> {
      return invoke<Issue>('issues_update', { folderPath, issueId, patch })
    },
    delete(folderPath: string, issueId: string): Promise<void> {
      return invoke<void>('issues_delete', { folderPath, issueId })
    },
    currentGitUser(folderPath: string): Promise<string> {
      return invoke<string>('issue_current_git_user', { folderPath })
    },
    epics: {
      create(folderPath: string, input: IssueEpicCreateInput): Promise<IssueEpic> {
        return invoke<IssueEpic>('issue_epics_create', { folderPath, input })
      },
      list(folderPath: string): Promise<IssueEpic[]> {
        return invoke<IssueEpic[]>('issue_epics_list', { folderPath })
      },
      get(folderPath: string, epicId: string): Promise<IssueEpic> {
        return invoke<IssueEpic>('issue_epics_get', { folderPath, epicId })
      },
      update(folderPath: string, epicId: string, patch: IssueEpicUpdateInput): Promise<IssueEpic> {
        return invoke<IssueEpic>('issue_epics_update', {
          folderPath,
          epicId,
          patch
        })
      },
      delete(folderPath: string, epicId: string): Promise<void> {
        return invoke<void>('issue_epics_delete', { folderPath, epicId })
      }
    },
    comments: {
      add(folderPath: string, input: IssueCommentCreateInput): Promise<IssueComment> {
        return invoke<IssueComment>('issue_comments_add', { folderPath, input })
      }
    }
  },

  tasks: {
    onChanged(handler: (folderPath: string) => void): Promise<Unsubscribe> {
      return getHostTransport().subscribe('tasks-changed', handler)
    },
    /** Return the parsed task list for a folder. Empty array when no `.sworm/tasks.jsonc` exists. */
    list(folderPath: string): Promise<TaskDefinition[]> {
      return invoke<TaskDefinition[]>('tasks_list', { folderPath })
    },
    /**
     * Start or attach a task PTY. `attachOnly` rejects unknown IDs instead of
     * executing again; `runId` identifies write/resize/stop calls.
     */
    start(
      runId: string,
      folderPath: string,
      taskId: string,
      activeFilePath: string | null,
      cols: number,
      rows: number,
      attachOnly: boolean,
      sinks: PtySinks
    ): StreamHandle<void> {
      return getHostTransport().openStream(
        {
          method: 'tasks_start',
          params: { runId, folderPath, taskId, activeFilePath, cols, rows, attachOnly }
        },
        sinks
      )
    },
    write(runId: string, data: Uint8Array): Promise<void> {
      return invoke<void>('tasks_write', { runId, data: Array.from(data) })
    },
    resize(runId: string, cols: number, rows: number): Promise<void> {
      return invoke<void>('tasks_resize', { runId, cols, rows })
    },
    stop(runId: string): Promise<void> {
      return invoke<void>('tasks_stop', { runId })
    }
  },

  formatting: {
    biome(folderPath: string, filePath: string, content: string): Promise<string> {
      return invoke<string>('formatting_format_biome', {
        folderPath,
        filePath,
        content
      })
    },
    nixfmt(folderPath: string, content: string): Promise<string> {
      return invoke<string>('formatting_format_nixfmt', { folderPath, content })
    }
  },

  lsp: {
    listServers(folderPath?: string): Promise<LspServerSettingsEntry[]> {
      return invoke<LspServerSettingsEntry[]>('lsp_list_servers', {
        folderPath: folderPath ?? null
      })
    },
    setServerConfig(config: LspServerConfig, folderPath?: string): Promise<LspServerConfig> {
      return invoke<LspServerConfig>('lsp_set_server_config', {
        config,
        folderPath: folderPath ?? null
      })
    },
    start(
      sessionId: string,
      folderPath: string,
      serverDefinitionId: string,
      rootPath: string,
      sinks: { onEvent: (event: LspEvent) => void }
    ): StreamHandle<void> {
      return getHostTransport().openStream(
        {
          method: 'lsp_start',
          params: { sessionId, folderPath, serverDefinitionId, rootPath }
        },
        sinks
      )
    },
    send(sessionId: string, messageJson: string): Promise<void> {
      return invoke<void>('lsp_send', { sessionId, messageJson })
    },
    stop(sessionId: string): Promise<void> {
      return invoke<void>('lsp_stop', { sessionId })
    }
  }
}
