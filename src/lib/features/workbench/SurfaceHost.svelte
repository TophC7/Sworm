<script lang="ts">
  import type { Tab } from '$lib/features/workbench/model'
  import LauncherSurface from '$lib/features/workbench/surfaces/launcher/LauncherSurface.svelte'
  import SessionSurface from '$lib/features/workbench/surfaces/session/SessionSurface.svelte'
  import TaskSurface from '$lib/features/workbench/surfaces/task/TaskSurface.svelte'
  import TextSurface from '$lib/features/workbench/surfaces/text/TextSurface.svelte'
  import DiffSurface from '$lib/features/workbench/surfaces/diff/DiffSurface.svelte'
  import ToolSurface from '$lib/features/workbench/surfaces/tool/ToolSurface.svelte'
  import IssueSurface from '$lib/features/workbench/surfaces/issue/IssueSurface.svelte'
  import EpicSurface from '$lib/features/workbench/surfaces/epic/EpicSurface.svelte'

  let { activeTab }: { activeTab: Tab } = $props()
</script>

{#if activeTab.kind === 'launcher'}
  <LauncherSurface folderPath={activeTab.folderPath} />
{:else if activeTab.kind === 'session'}
  <SessionSurface tab={activeTab} />
{:else if activeTab.kind === 'text'}
  <TextSurface tab={activeTab} folderPath={activeTab.folderPath} locked={activeTab.locked} />
{:else if activeTab.kind === 'diff'}
  <DiffSurface tab={activeTab} folderPath={activeTab.folderPath} />
{:else if activeTab.kind === 'tool'}
  <ToolSurface tab={activeTab} />
{:else if activeTab.kind === 'task'}
  <TaskSurface tab={activeTab} folderPath={activeTab.folderPath} />
{:else if activeTab.kind === 'issue'}
  <IssueSurface tab={activeTab} folderPath={activeTab.folderPath} />
{:else if activeTab.kind === 'epic'}
  <EpicSurface tab={activeTab} folderPath={activeTab.folderPath} />
{/if}
