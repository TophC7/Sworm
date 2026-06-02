// Typed frontend/backend bridge.
//
// Every Tauri invoke call goes through this module. Pages and
// components must NOT import invoke() directly.

import { Channel, invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  AppInfo,
  BranchOpState,
  BranchSummary,
  BuiltinCatalog,
  CommitDetail,
  ConfigSchemaEntry,
  DiffFileContent,
  DiffSource,
  DiscoveredProject,
  EnvProbeResult,
  EffectiveSettingsPayload,
  FileDiff,
  FileEntryStat,
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
  Project,
  ProjectSessionGroup,
  ProviderConfig,
  ProviderStatus,
  PtyEvent,
  Session,
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

  projects: {
    selectDirectory(): Promise<string | null> {
      return invoke<string | null>('project_select_directory')
    },
    add(path: string): Promise<Project> {
      return invoke<Project>('project_add', { path })
    },
    /**
     * Re-probe git for a project and persist the resolved branch +
     * base ref. Called immediately after `add` to backfill metadata
     * that the synchronous insert deliberately skips.
     */
    refreshGit(id: string): Promise<Project> {
      return invoke<Project>('project_refresh_git', { id })
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
      return invoke<ProviderStatus[]>('provider_list_for_project', {
        projectId
      })
    }
  },

  sessions: {
    create(projectId: string, providerId: string, title: string): Promise<Session> {
      return invoke<Session>('session_create', {
        projectId,
        providerId,
        title
      })
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
      return invoke<void>('session_start', {
        sessionId,
        cols,
        rows,
        output,
        events
      })
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
      return invoke<void>('session_write', {
        sessionId,
        data: Array.from(data)
      })
    },
    resize(sessionId: string, cols: number, rows: number): Promise<void> {
      return invoke<void>('session_resize', { sessionId, cols, rows })
    },
    stop(sessionId: string): Promise<void> {
      return invoke<void>('session_stop', { sessionId })
    },
    reset(sessionId: string): Promise<void> {
      return invoke<void>('session_reset', { sessionId })
    },
    remove(sessionId: string): Promise<void> {
      return invoke<void>('session_remove', { sessionId })
    },
    archive(sessionId: string): Promise<void> {
      return invoke<void>('session_archive', { sessionId })
    },
    unarchive(sessionId: string): Promise<void> {
      return invoke<void>('session_unarchive', { sessionId })
    },
    listArchived(projectId: string): Promise<Session[]> {
      return invoke<Session[]>('session_list_archived', { projectId })
    },
    listProjectGroups(projectIds: string[]): Promise<ProjectSessionGroup[]> {
      return invoke<ProjectSessionGroup[]>('session_list_project_groups', { projectIds })
    },
    /**
     * Return whether the backend currently holds a live PTY for this
     * session. Distinct from `status`; survives the truth even after
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
      info(path: string, name: string): Promise<BranchSummary> {
        return invoke<BranchSummary>('git_branch_info', { path, name })
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

  fresh: {
    openFile(projectId: string, projectPath: string, filePath: string): Promise<void> {
      return invoke<void>('editor_open_file', {
        projectId,
        projectPath,
        filePath
      })
    },
    openAtCommit(projectId: string, projectPath: string, commitHash: string, filePath: string): Promise<void> {
      return invoke<void>('editor_open_at_commit', {
        projectId,
        projectPath,
        commitHash,
        filePath
      })
    },
    openAtStash(projectId: string, projectPath: string, stashIndex: number, filePath: string): Promise<void> {
      return invoke<void>('editor_open_at_stash', {
        projectId,
        projectPath,
        stashIndex,
        filePath
      })
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
    stat(projectPath: string, filePath: string): Promise<FileEntryStat | null> {
      return invoke<FileEntryStat | null>('file_stat', {
        projectPath,
        filePath
      })
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
    getEffective(projectPath?: string): Promise<EffectiveSettingsPayload> {
      return invoke<EffectiveSettingsPayload>('settings_get_effective', {
        input: { project_path: projectPath ?? null }
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
    createProjectFile(projectPath: string): Promise<SettingsFileResult> {
      return invoke<SettingsFileResult>('settings_create_project_file', {
        input: { project_path: projectPath }
      })
    },
    openProjectFile(projectPath: string): Promise<SettingsFileResult> {
      return invoke<SettingsFileResult>('settings_open_project_file', {
        input: { project_path: projectPath }
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
    list(projectId: string, filters: IssueListFilters = {}): Promise<Issue[]> {
      return invoke<Issue[]>('issues_list', { projectId, filters })
    },
    ready(projectId: string, filters: IssueReadyFilters | number = {}): Promise<Issue[]> {
      const readyFilters = typeof filters === 'number' ? { limit: filters } : filters
      return invoke<Issue[]>('issues_ready', {
        projectId,
        filters: readyFilters
      })
    },
    search(projectId: string, query: string, filters: IssueSearchFilters = {}): Promise<Issue[]> {
      return invoke<Issue[]>('issues_search', { projectId, query, filters })
    },
    get(projectId: string, issueId: string): Promise<IssueDetail> {
      return invoke<IssueDetail>('issues_get', { projectId, issueId })
    },
    create(projectId: string, input: IssueCreateInput): Promise<Issue> {
      return invoke<Issue>('issues_create', { projectId, input })
    },
    update(projectId: string, issueId: string, patch: IssueUpdateInput): Promise<Issue> {
      return invoke<Issue>('issues_update', { projectId, issueId, patch })
    },
    delete(projectId: string, issueId: string): Promise<void> {
      return invoke<void>('issues_delete', { projectId, issueId })
    },
    currentGitUser(projectId: string): Promise<string> {
      return invoke<string>('issue_current_git_user', { projectId })
    },
    epics: {
      create(projectId: string, input: IssueEpicCreateInput): Promise<IssueEpic> {
        return invoke<IssueEpic>('issue_epics_create', { projectId, input })
      },
      list(projectId: string): Promise<IssueEpic[]> {
        return invoke<IssueEpic[]>('issue_epics_list', { projectId })
      },
      get(projectId: string, epicId: string): Promise<IssueEpic> {
        return invoke<IssueEpic>('issue_epics_get', { projectId, epicId })
      },
      update(projectId: string, epicId: string, patch: IssueEpicUpdateInput): Promise<IssueEpic> {
        return invoke<IssueEpic>('issue_epics_update', {
          projectId,
          epicId,
          patch
        })
      },
      delete(projectId: string, epicId: string): Promise<void> {
        return invoke<void>('issue_epics_delete', { projectId, epicId })
      }
    },
    comments: {
      add(projectId: string, input: IssueCommentCreateInput): Promise<IssueComment> {
        return invoke<IssueComment>('issue_comments_add', { projectId, input })
      },
      list(projectId: string, issueId: string): Promise<IssueComment[]> {
        return invoke<IssueComment[]>('issue_comments_list', {
          projectId,
          issueId
        })
      },
      update(projectId: string, commentId: string, input: IssueCommentUpdateInput): Promise<IssueComment> {
        return invoke<IssueComment>('issue_comments_update', {
          projectId,
          commentId,
          input
        })
      },
      delete(projectId: string, commentId: string): Promise<void> {
        return invoke<void>('issue_comments_delete', { projectId, commentId })
      }
    },
    dependencies: {
      add(projectId: string, input: IssueDependencyInput): Promise<IssueDependency> {
        return invoke<IssueDependency>('issue_dependencies_add', {
          projectId,
          input
        })
      },
      remove(projectId: string, input: IssueDependencyInput): Promise<void> {
        return invoke<void>('issue_dependencies_remove', { projectId, input })
      },
      list(projectId: string, issueId: string): Promise<IssueDependency[]> {
        return invoke<IssueDependency[]>('issue_dependencies_list', {
          projectId,
          issueId
        })
      }
    },
    config: {
      list(projectId: string): Promise<IssueConfigEntry[]> {
        return invoke<IssueConfigEntry[]>('issue_config_list', { projectId })
      },
      get(projectId: string, key: string): Promise<IssueConfigEntry | null> {
        return invoke<IssueConfigEntry | null>('issue_config_get', {
          projectId,
          key
        })
      },
      set(projectId: string, key: string, value: string): Promise<IssueConfigEntry> {
        return invoke<IssueConfigEntry>('issue_config_set', {
          projectId,
          key,
          value
        })
      }
    }
  },

  tasks: {
    /** Return the parsed task list for a project. Empty array when no `.sworm/tasks.json` exists. */
    list(projectId: string): Promise<TaskDefinition[]> {
      return invoke<TaskDefinition[]>('tasks_list', { projectId })
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
      projectId: string,
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
        projectId,
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
    biome(projectId: string, filePath: string, content: string): Promise<string> {
      return invoke<string>('formatting_format_biome', {
        projectId,
        filePath,
        content
      })
    },
    nixfmt(projectId: string, content: string): Promise<string> {
      return invoke<string>('formatting_format_nixfmt', { projectId, content })
    }
  },

  lsp: {
    listServers(projectId?: string): Promise<LspServerSettingsEntry[]> {
      return invoke<LspServerSettingsEntry[]>('lsp_list_servers', {
        projectId: projectId ?? null
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
      projectId: string,
      serverDefinitionId: string,
      rootPath: string,
      onEvent: (event: LspEvent) => void
    ): Promise<void> {
      const events = backend.lsp.createEventChannel(onEvent)
      return invoke<void>('lsp_start', {
        sessionId,
        projectId,
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
