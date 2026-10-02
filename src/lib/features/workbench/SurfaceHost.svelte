<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import { getTabGroup, retryGroup } from '$lib/features/workbench/groups.svelte'
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
  let group = $derived(getTabGroup(activeTab))
</script>

<!-- Busy/revoked groups own no tabs; only offline and attaching reach here. -->
{#if group && group.state !== 'active'}
  <div class="flex h-full flex-col items-center justify-center gap-3 bg-ground p-6 text-center">
    <p class="text-base text-muted">
      {group.state === 'offline' ? `${group.server} is offline` : `Attaching to ${group.server}…`}
    </p>
    {#if group.state === 'offline'}
      <Button variant="accent" onclick={() => void retryGroup(group.server)}>Retry</Button>
    {/if}
  </div>
{:else if activeTab.kind === 'launcher'}
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
