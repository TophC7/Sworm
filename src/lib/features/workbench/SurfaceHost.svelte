<script lang="ts">
  import type { Session } from '$lib/types/backend'
  import type { Tab } from '$lib/features/workbench/model'
  import LauncherSurface from '$lib/features/workbench/surfaces/launcher/LauncherSurface.svelte'
  import SessionSurface from '$lib/features/workbench/surfaces/session/SessionSurface.svelte'
  import TaskSurface from '$lib/features/workbench/surfaces/task/TaskSurface.svelte'
  import TextSurface from '$lib/features/workbench/surfaces/text/TextSurface.svelte'
  import DiffSurface from '$lib/features/workbench/surfaces/diff/DiffSurface.svelte'
  import ToolSurface from '$lib/features/workbench/surfaces/tool/ToolSurface.svelte'

  let {
    activeTab = null,
    paneSession = null,
    projectId,
    projectPath,
    onSessionCreated,
    onSessionStatusChange
  }: {
    activeTab: Tab | null
    paneSession: Session | null
    projectId: string
    projectPath: string
    onSessionCreated?: () => void
    onSessionStatusChange?: (status: Session['status']) => void
  } = $props()
</script>

{#if !activeTab || activeTab.kind === 'launcher'}
  <LauncherSurface onCreated={onSessionCreated} />
{:else if activeTab.kind === 'session' && paneSession}
  <SessionSurface
    session={paneSession}
    {projectId}
    {projectPath}
    locked={activeTab.locked}
    onStatusChange={onSessionStatusChange}
  />
{:else if activeTab.kind === 'text'}
  <TextSurface tab={activeTab} {projectId} {projectPath} locked={activeTab.locked} />
{:else if activeTab.kind === 'diff'}
  <DiffSurface tab={activeTab} {projectId} {projectPath} />
{:else if activeTab.kind === 'tool'}
  <ToolSurface tab={activeTab} />
{:else if activeTab.kind === 'task'}
  <TaskSurface tab={activeTab} {projectId} />
{/if}
