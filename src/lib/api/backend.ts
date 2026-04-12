// Typed frontend/backend bridge.
//
// Every Tauri invoke call goes through this module. Pages and
// components must NOT import invoke() directly.

import { invoke, Channel } from '@tauri-apps/api/core'
import type {
  AppInfo,
  DiffContext,
  EnvProbeResult,
  GitCommit,
  GitSummary,
  GeneralSettings,
  NixDetection,
  NixEnvRecord,
  Project,
  ProviderStatus,
  PtyEvent,
  Session,
  SettingsPayload,
  ProviderConfig
} from '$lib/types/backend'

export const backend = {
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
    }
  },

  projects: {
    selectDirectory(): Promise<string | null> {
      return invoke<string | null>('project_select_directory')
    },
    add(path: string): Promise<Project> {
      return invoke<Project>('project_add', { path })
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
    }
  },

  git: {
    getSummary(path: string): Promise<GitSummary> {
      return invoke<GitSummary>('git_get_summary', { path })
    },
    getFileDiff(projectPath: string, filePath: string, staged: boolean): Promise<string | null> {
      return invoke<string | null>('git_get_file_diff', { projectPath, filePath, staged })
    },
    getDiffContext(projectPath: string, filePath: string, staged: boolean): Promise<DiffContext | null> {
      return invoke<DiffContext | null>('git_get_diff_context', { projectPath, filePath, staged })
    },
    getLog(path: string, limit = 20): Promise<GitCommit[]> {
      return invoke<GitCommit[]>('git_get_log', { path, limit })
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
