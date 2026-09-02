// Typed frontend/backend bridge.
//
// Every Tauri invoke call goes through this module. Pages and
// components must NOT import invoke() directly.

import { Channel, invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  BranchOpState,
  BranchSummary,
  BuiltinCatalog,
  CommitDetail,
  ConfigSchemaEntry,
  DiffFileContent,
  DiffSource,
  DiscoveredProject,
  EffectiveSettingsPayload,
  FileDiff,
  FilePasteCollision,
  FormattingSettings,
  GeneralSettings,
  GitQuickDiffData,
  GitSummary,
  GraphCommit,
  Issue,
  IssueComment,
  IssueCommentCreateInput,
  IssueCommentUpdateInput,
  IssueConfigEntry,
  IssueCreateInput,
  IssueDependency,
  IssueDependencyInput,
  IssueDetail,
  IssueEpic,
  IssueEpicCreateInput,
  IssueEpicUpdateInput,
  IssueListFilters,
  IssueReadyFilters,
  IssueSearchFilters,
  IssueUpdateInput,
  LspEvent,
  LspServerConfig,
  LspServerSettingsEntry,
  NixDetection,
  NixDiagnostic,
  NixEnvRecord,
  FolderInfo,
  ProviderConfig,
  ProviderStatus,
  PtyEvent,
  SessionSpec,
  SessionStartInfo,
  SettingsChangedEvent,
  SettingsFileResult,
  SettingsLayerPayload,
  SettingsPayload,
  ShortcutsFilePayload,
  ShortcutsFileResult,
  StashEntry,
  TaskDefinition
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
    stateGet(key: string): Promise<string | null> {
      return invoke<string | null>('app_state_get', { key })
    },
    statePut(key: string, valueJson: string): Promise<void> {
      return invoke<void>('app_state_put', { key, valueJson })
    },
    /** Copy file paths to the system clipboard in file-manager format. */
    clipboardCopyFiles(paths: string[], op: 'copy' | 'cut'): Promise<void> {
      return invoke<void>('clipboard_copy_files', { paths, op })
    },
    /** Read file URIs from the system clipboard. Returns null if none. */
    clipboardReadFiles(): Promise<{
      op: 'copy' | 'cut'
      paths: string[]
    } | null> {
      return invoke<{ op: 'copy' | 'cut'; paths: string[] } | null>('clipboard_read_files')
    },
    /**
     * Drain the "open this folder" path queued by argv (Nautilus "Open With"
     * or CLI invocation). Returns null when nothing is queued.
     */
    takePendingOpenPath(): Promise<string | null> {
      return invoke<string | null>('app_take_pending_open_path')
    }
  },

  dnd: {
    saveDroppedBytes(bytes: Uint8Array, suggestedName: string): Promise<string> {
      return invoke<string>('dnd_save_dropped_bytes', {
        bytes: Array.from(bytes),
        suggestedName
      })
    }
  },

  folders: {
    selectDirectory(): Promise<string | null> {
      return invoke<string | null>('folder_select_directory')
    },
    /** Canonicalize a folder path; rejects missing paths and non-directories. */
    resolve(path: string): Promise<FolderInfo> {
      return invoke<FolderInfo>('folder_resolve', { path })
    },
    openInTerminal(path: string): Promise<void> {
      return invoke<void>('folder_open_in_terminal', { path })
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
    /**
     * Spawn the provider process for a session tab under a fresh `runId`.
     * `resumeToken` is the provider conversation identity known from a
     * previous run; the backend validates it and reports `resumed` plus
     * the token the launched process owns at spawn time (null until
     * discovery binds one).
     */
    start(
      spec: SessionSpec,
      cols: number,
      rows: number,
      output: Channel<number[]>,
      events: Channel<PtyEvent>
    ): Promise<SessionStartInfo> {
      return invoke<SessionStartInfo>('session_start', {
        runId: spec.runId,
        folderPath: spec.folderPath,
        providerId: spec.providerId,
        resumeToken: spec.resumeToken,
        cols,
        rows,
        output,
        events
      })
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
      return invoke<CommitDetail | null>('git_get_commit_detail', {
        path,
        hash
      })
    },
    /**
     * Unified payload for the Monaco multi-file diff viewer.
     * Returns one `FileDiff` per changed file with both sides of
     * content attached. Source-agnostic (working / commit / stash).
     */
    getDiffFiles(path: string, source: DiffSource): Promise<FileDiff[]> {
      return invoke<FileDiff[]>('diff_get_files', { path, source })
    },
    /**
     * Cheap working-tree diff index; file list + metadata only,
     * no content. Pair with [`getWorkingDiffFile`] to load each
     * file lazily so a 200-file working tree doesn't ship a
     * multi-megabyte payload before the user has expanded any row.
     */
    getWorkingDiffIndex(path: string, staged: boolean): Promise<FileDiff[]> {
      return invoke<FileDiff[]>('diff_get_working_index', { path, staged })
    },
    getWorkingDiffFile(
      path: string,
      filePath: string,
      status: FileDiff['status'],
      staged: boolean
    ): Promise<DiffFileContent> {
      return invoke<DiffFileContent>('diff_get_working_file', {
        path,
        filePath,
        status,
        staged
      })
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
    },

    /**
     * Branch read + write surface. Each method maps one-to-one onto a
     * `git_*` Tauri command in `src-tauri/src/commands/git.rs`. All
     * mutating calls route through `run_mutate` server-side, so the
     * summary cache invalidates and the StatusBar reflects the new
     * state inside one poll cycle.
     */
    branch: {
      list(path: string): Promise<BranchSummary[]> {
        return invoke<BranchSummary[]>('git_list_branches', { path })
      },
      commits(path: string, branch: string, limit = 5): Promise<GraphCommit[]> {
        return invoke<GraphCommit[]>('git_get_branch_commits', {
          path,
          branch,
          limit
        })
      },
      status(path: string): Promise<BranchOpState> {
        return invoke<BranchOpState>('git_branch_status', { path })
      },
      diffAgainstHead(path: string, branch: string): Promise<FileDiff[]> {
        return invoke<FileDiff[]>('git_diff_branch_against_head', {
          path,
          branch
        })
      },
      checkout(path: string, name: string): Promise<void> {
        return invoke<void>('git_checkout_branch', { path, name })
      },
      checkoutRemoteAsLocal(path: string, remoteName: string, localName: string): Promise<void> {
        return invoke<void>('git_checkout_remote_as_local', {
          path,
          remoteName,
          localName
        })
      },
      create(path: string, name: string, base: string, opts: { checkout?: boolean } = {}): Promise<void> {
        return invoke<void>('git_create_branch', {
          path,
          name,
          base,
          checkout: opts.checkout ?? false
        })
      },
      rename(path: string, oldName: string, newName: string): Promise<void> {
        return invoke<void>('git_rename_branch', { path, oldName, newName })
      },
      delete(path: string, name: string, opts: { force?: boolean } = {}): Promise<void> {
        return invoke<void>('git_delete_branch', {
          path,
          name,
          force: opts.force ?? false
        })
      },
      deleteRemote(path: string, remote: string, name: string): Promise<void> {
        return invoke<void>('git_delete_remote_branch', { path, remote, name })
      },
      setUpstream(path: string, branch: string, upstream: string): Promise<void> {
        return invoke<void>('git_set_upstream', { path, branch, upstream })
      },
      fastForward(path: string, name: string): Promise<void> {
        return invoke<void>('git_fast_forward_branch', { path, name })
      },
      merge(path: string, source: string, opts: { noFf?: boolean } = {}): Promise<void> {
        return invoke<void>('git_merge_into_current', {
          path,
          source,
          noFf: opts.noFf ?? false
        })
      },
      rebaseOnto(path: string, target: string): Promise<void> {
        return invoke<void>('git_rebase_current_onto', { path, target })
      },
      rebaseContinue(path: string): Promise<void> {
        return invoke<void>('git_rebase_continue', { path })
      },
      rebaseSkip(path: string): Promise<void> {
        return invoke<void>('git_rebase_skip', { path })
      },
      rebaseAbort(path: string): Promise<void> {
        return invoke<void>('git_rebase_abort', { path })
      },
      mergeAbort(path: string): Promise<void> {
        return invoke<void>('git_merge_abort', { path })
      }
    }
  },

  editor: {
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
      return invoke<FilePasteCollision[]>('file_paste_collisions', {
        projectPath,
        targetDir,
        sources
      })
    }
  },

  nix: {
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
    get(): Promise<SettingsPayload> {
      return invoke<SettingsPayload>('settings_get')
    },
    getEffective(folderPath?: string): Promise<EffectiveSettingsPayload> {
      return invoke<EffectiveSettingsPayload>('settings_get_effective', {
        input: { folder_path: folderPath ?? null }
      })
    },
    getGlobalLayer(): Promise<SettingsLayerPayload> {
      return invoke<SettingsLayerPayload>('settings_get_global_layer')
    },
    patchGlobalSection(
      section: 'general' | 'formatting' | 'providers' | 'lsp',
      value: unknown
    ): Promise<SettingsLayerPayload> {
      return invoke<SettingsLayerPayload>('settings_patch_global_section', {
        input: { section, value }
      })
    },
    createGlobalFile(): Promise<SettingsFileResult> {
      return invoke<SettingsFileResult>('settings_create_global_file')
    },
    openGlobalFile(): Promise<SettingsFileResult> {
      return invoke<SettingsFileResult>('settings_open_global_file')
    },
    openFolderFile(folderPath: string): Promise<SettingsFileResult> {
      return invoke<SettingsFileResult>('settings_open_folder_file', {
        input: { folder_path: folderPath }
      })
    },
    onChanged(handler: (event: SettingsChangedEvent) => void): Promise<UnlistenFn> {
      return listen<SettingsChangedEvent>('settings-changed', (event) => handler(event.payload))
    },
    setGeneral(settings: GeneralSettings): Promise<GeneralSettings> {
      return invoke<GeneralSettings>('settings_set_general', { settings })
    },
    setFormatting(formatting: FormattingSettings): Promise<FormattingSettings> {
      return invoke<FormattingSettings>('settings_set_formatting', {
        formatting
      })
    },
    setProviderConfig(config: ProviderConfig): Promise<ProviderConfig> {
      return invoke<ProviderConfig>('settings_set_provider_config', { config })
    }
  },

  shortcuts: {
    getGlobal(): Promise<ShortcutsFilePayload> {
      return invoke<ShortcutsFilePayload>('shortcuts_get_global')
    },
    setGlobal(value: unknown): Promise<ShortcutsFilePayload> {
      return invoke<ShortcutsFilePayload>('shortcuts_set_global', { value })
    },
    createGlobalFile(): Promise<ShortcutsFileResult> {
      return invoke<ShortcutsFileResult>('shortcuts_create_global_file')
    },
    openGlobalFile(): Promise<ShortcutsFileResult> {
      return invoke<ShortcutsFileResult>('shortcuts_open_global_file')
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
    list(folderPath: string, filters: IssueListFilters = {}): Promise<Issue[]> {
      return invoke<Issue[]>('issues_list', { folderPath, filters })
    },
    ready(folderPath: string, filters: IssueReadyFilters | number = {}): Promise<Issue[]> {
      const readyFilters = typeof filters === 'number' ? { limit: filters } : filters
      return invoke<Issue[]>('issues_ready', {
        folderPath,
        filters: readyFilters
      })
    },
    search(folderPath: string, query: string, filters: IssueSearchFilters = {}): Promise<Issue[]> {
      return invoke<Issue[]>('issues_search', { folderPath, query, filters })
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
      },
      list(folderPath: string, issueId: string): Promise<IssueComment[]> {
        return invoke<IssueComment[]>('issue_comments_list', {
          folderPath,
          issueId
        })
      },
      update(folderPath: string, commentId: string, input: IssueCommentUpdateInput): Promise<IssueComment> {
        return invoke<IssueComment>('issue_comments_update', {
          folderPath,
          commentId,
          input
        })
      },
      delete(folderPath: string, commentId: string): Promise<void> {
        return invoke<void>('issue_comments_delete', { folderPath, commentId })
      }
    },
    dependencies: {
      add(folderPath: string, input: IssueDependencyInput): Promise<IssueDependency> {
        return invoke<IssueDependency>('issue_dependencies_add', {
          folderPath,
          input
        })
      },
      remove(folderPath: string, input: IssueDependencyInput): Promise<void> {
        return invoke<void>('issue_dependencies_remove', { folderPath, input })
      },
      list(folderPath: string, issueId: string): Promise<IssueDependency[]> {
        return invoke<IssueDependency[]>('issue_dependencies_list', {
          folderPath,
          issueId
        })
      }
    },
    config: {
      list(folderPath: string): Promise<IssueConfigEntry[]> {
        return invoke<IssueConfigEntry[]>('issue_config_list', { folderPath })
      }
    }
  },

  tasks: {
    /** Return the parsed task list for a folder. Empty array when no `.sworm/tasks.json` exists. */
    list(folderPath: string): Promise<TaskDefinition[]> {
      return invoke<TaskDefinition[]>('tasks_list', { folderPath })
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
    /**
     * Spawn a task in a PTY. `runId` is a frontend-generated UUID used
     * as the PTY key for subsequent write/resize/stop calls.
     */
    start(
      runId: string,
      folderPath: string,
      taskId: string,
      activeFilePath: string | null,
      cols: number,
      rows: number,
      onOutput: (data: number[]) => void,
      onEvent: (event: PtyEvent) => void
    ): Promise<void> {
      const output = backend.tasks.createOutputChannel(onOutput)
      const events = backend.tasks.createEventChannel(onEvent)
      return invoke<void>('tasks_start', {
        runId,
        folderPath,
        taskId,
        activeFilePath,
        cols,
        rows,
        output,
        events
      })
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
    setServerConfig(config: LspServerConfig): Promise<LspServerConfig> {
      return invoke<LspServerConfig>('lsp_set_server_config', { config })
    },
    createEventChannel(onEvent: (event: LspEvent) => void): Channel<LspEvent> {
      const events = new Channel<LspEvent>()
      events.onmessage = onEvent
      return events
    },
    start(
      sessionId: string,
      folderPath: string,
      serverDefinitionId: string,
      rootPath: string,
      onEvent: (event: LspEvent) => void
    ): Promise<void> {
      const events = backend.lsp.createEventChannel(onEvent)
      return invoke<void>('lsp_start', {
        sessionId,
        folderPath,
        serverDefinitionId,
        rootPath,
        events
      })
    },
    send(sessionId: string, messageJson: string): Promise<void> {
      return invoke<void>('lsp_send', { sessionId, messageJson })
    },
    stop(sessionId: string): Promise<void> {
      return invoke<void>('lsp_stop', { sessionId })
    }
  }
}
