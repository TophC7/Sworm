<script lang="ts">
  import { onMount } from 'svelte'
  import ActivityHeatmap from './ActivityHeatmap.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { InfoTooltip } from '$lib/components/ui/tooltip'
  import { BlurFade } from '$lib/components/ui/blur-fade'
  import { allProviders } from '$lib/features/sessions/providers/catalog'
  import { addProject } from '$lib/features/projects/state.svelte'
  import { openProject } from '$lib/features/workbench/state.svelte'
  import {
    getDiscoveredProjects,
    isActivityMapLoading,
    loadActivityMap,
    refreshActivityMap
  } from '$lib/features/activity-map/state.svelte'
  import { timeAgo } from '$lib/utils/date'
  import { parentPath } from '$lib/utils/paths'
  import { RefreshCw } from '$lib/icons/lucideExports'
  import type { DiscoveredProject } from '$lib/types/backend'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'

  // Provider icon lookup
  const providerMap = new Map(allProviders.map((p) => [p.id, p]))

  let projects = $derived(getDiscoveredProjects())
  let loading = $derived(isActivityMapLoading())

  // Only show projects not already in Sworm
  let externalProjects = $derived(projects.filter((p) => !p.is_sworm_project))

  onMount(() => {
    loadActivityMap()
  })

  async function handleOpen(project: DiscoveredProject) {
    if (!project.path_exists) return
    if (project.sworm_project_id) {
      openProject(project.sworm_project_id)
      return
    }
    try {
      const added = await addProject(project.path)
      openProject(added.id)
    } catch (e) {
      notify.error('Open project failed', getErrorMessage(e))
    }
  }

  async function handleRefresh() {
    try {
      await refreshActivityMap()
    } catch (error) {
      notify.error('Rescan activity failed', getErrorMessage(error))
    }
  }

  // Merge all daily_counts across providers for a project's heatmap
  function mergedCounts(project: DiscoveredProject): number[] {
    const merged = [0, 0, 0, 0, 0, 0, 0]
    for (const prov of project.providers) {
      for (let i = 0; i < 7; i++) {
        merged[i] += prov.daily_counts[i]
      }
    }
    return merged
  }
</script>

{#if externalProjects.length > 0}
  <div class="mt-8 mb-2">
    <div class="mb-3 flex items-center gap-2">
      <h2 class="text-xs tracking-widest text-muted uppercase">Activity</h2>
      <InfoTooltip ariaLabel="What is Activity?">
        <p class="max-w-56 text-sm leading-snug">
          Projects where you've used coding agents on this machine. Scanned locally from CLI history — nothing leaves
          your device.
        </p>
      </InfoTooltip>
      <IconButton tooltip="Rescan agent history" onclick={handleRefresh} disabled={loading}>
        <RefreshCw size={11} class={loading ? 'animate-spin' : ''} />
      </IconButton>
    </div>

    <div class="scrollbar-none flex gap-2.5 overflow-x-auto pb-1">
      {#each externalProjects.slice(0, 12) as project, i (project.path)}
        <BlurFade delay={0.1 + i * 0.06} duration={0.35} direction="up" offset={6}>
          <button
            class="group flex w-44 shrink-0 cursor-pointer flex-col gap-2 rounded-xl border border-edge bg-raised px-3.5 py-3 text-left transition-colors hover:border-accent/40 {!project.path_exists
              ? 'opacity-40 grayscale'
              : ''}"
            onclick={() => handleOpen(project)}
            title={project.path}
            disabled={!project.path_exists}
          >
            <div class="w-full min-w-0">
              <div class="truncate text-base font-medium text-fg transition-colors group-hover:text-bright">
                {project.name}
              </div>
              <div class="truncate text-2xs text-subtle">
                {parentPath(project.path)}
              </div>
            </div>

            <ActivityHeatmap counts={mergedCounts(project)} />

            <div class="flex w-full items-center justify-between">
              <div class="flex items-center gap-1">
                {#each project.providers as prov}
                  {@const meta = providerMap.get(prov.provider_id)}
                  {#if meta}
                    <img src={meta.icon} alt={meta.label} title={meta.label} class="h-3.5 w-3.5 rounded-sm" />
                  {/if}
                {/each}
              </div>
              <span class="text-2xs text-subtle">
                {timeAgo(project.last_active)}
              </span>
            </div>
          </button>
        </BlurFade>
      {/each}
    </div>
  </div>
{/if}
