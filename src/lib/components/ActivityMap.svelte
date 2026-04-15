<script lang="ts">
  import { onMount } from 'svelte'
  import ActivityHeatmap from './ActivityHeatmap.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { InfoTooltip } from '$lib/components/ui/tooltip'
  import { BlurFade } from '$lib/components/ui/blur-fade'
  import { allProviders } from '$lib/data/providers'
  import { addProject } from '$lib/stores/projects.svelte'
  import { openProject } from '$lib/stores/workspace.svelte'
  import {
    getDiscoveredProjects,
    isActivityMapLoading,
    loadActivityMap,
    refreshActivityMap
  } from '$lib/stores/activityMap.svelte'
  import { timeAgo } from '$lib/utils/date'
  import { parentPath } from '$lib/utils/path'
  import { RefreshCw } from '$lib/icons/lucideExports'
  import type { DiscoveredProject } from '$lib/types/backend'

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
      console.error('Failed to open discovered project:', e)
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
      <h2 class="text-[0.7rem] tracking-widest text-muted uppercase">Activity</h2>
      <InfoTooltip ariaLabel="What is Activity?">
        <p class="max-w-56 text-[0.75rem] leading-snug">
          Projects where you've used coding agents on this machine. Scanned locally from CLI history — nothing leaves
          your device.
        </p>
      </InfoTooltip>
      <IconButton tooltip="Rescan agent history" onclick={() => refreshActivityMap()} disabled={loading}>
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
              <div class="truncate text-[0.8rem] font-medium text-fg transition-colors group-hover:text-bright">
                {project.name}
              </div>
              <div class="truncate text-[0.65rem] text-subtle">
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
              <span class="text-[0.6rem] text-subtle">
                {timeAgo(project.last_active)}
              </span>
            </div>
          </button>
        </BlurFade>
      {/each}
    </div>
  </div>
{/if}
