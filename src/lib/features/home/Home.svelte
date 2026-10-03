<!--
  @component
  Home — what an empty window or workbench shows. Continue lists server
  workbenches on desktop and other workbenches on web; Projects merges
  opened folders with agent CLI activity, newest first.
-->

<script lang="ts">
  import { onMount, type Snippet } from 'svelte'
  import { SvelteMap } from 'svelte/reactivity'
  import { backend } from '$lib/api/backend'
  import StageView from '$lib/components/layout/StageView.svelte'
  import { BlurFade } from '$lib/components/ui/blur-fade'
  import { Button, IconButton } from '$lib/components/ui/button'
  import { InfoTooltip } from '$lib/components/ui/tooltip'
  import {
    getDiscoveredProjects,
    isActivityMapLoading,
    loadActivityMap,
    refreshActivityMap
  } from '$lib/features/activity-map/state.svelte'
  import { getRecentFolders } from '$lib/features/folders/state.svelte'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import { RefreshCw } from '$lib/icons/lucideExports'
  import appIconUrl from '$lib/assets/sworm.svg?url'
  import type { GitBrief, WorkbenchInfo } from '$lib/types/backend'
  import ContinueRow from './ContinueRow.svelte'
  import ProjectCard from './ProjectCard.svelte'
  import { mergeProjects } from './projects'

  const COLLAPSED_PROJECTS = 9
  // "Show all" can list every known project; each probe spawns a `git status`.
  const MAX_BRIEF_REQUESTS = 4

  let {
    onOpenProject,
    actions,
    sections,
    closing,
    closeErrors,
    onOpenWorkbench,
    onTakeOverWorkbench,
    onCloseWorkbench
  }: {
    onOpenProject: (path: string) => void
    /** Start controls beside the wordmark. */
    actions?: Snippet
    sections: { server?: string; workbenches: WorkbenchInfo[] }[]
    closing?: ReadonlySet<string>
    closeErrors?: Record<string, string>
    onOpenWorkbench?: (server: string, workbench: WorkbenchInfo) => void
    onTakeOverWorkbench: (workbench: WorkbenchInfo, server?: string) => void
    onCloseWorkbench?: (workbench: WorkbenchInfo, server?: string) => void
  } = $props()

  let projects = $derived(mergeProjects(getRecentFolders(), getDiscoveredProjects()))
  let expanded = $state(false)
  let visible = $derived(expanded ? projects : projects.slice(0, COLLAPSED_PROJECTS))
  let scanning = $derived(isActivityMapLoading())
  let highlightPath = $state<string | null>(null)
  let hasContinue = $derived(sections.some(({ workbenches }) => workbenches.length > 0))

  let openCounts = $derived.by(() => {
    const counts = new Map<string, number>()
    for (const workbench of sections.flatMap(({ workbenches }) => workbenches)) {
      for (const folder of workbench.folders) counts.set(folder, (counts.get(folder) ?? 0) + 1)
    }
    return counts
  })

  // Briefs load once per card for this visit; `null` marks a failed probe.
  const briefs = new SvelteMap<string, GitBrief | null>()
  const requested = new Set<string>()
  const queue: string[] = []
  let inFlight = 0

  function drainQueue(): void {
    while (inFlight < MAX_BRIEF_REQUESTS) {
      const path = queue.shift()
      if (path === undefined) return
      inFlight++
      backend.git
        .getBrief(path)
        .then(
          (brief) => briefs.set(path, brief),
          () => briefs.set(path, null)
        )
        .finally(() => {
          inFlight--
          drainQueue()
        })
    }
  }

  $effect(() => {
    for (const { path, exists } of visible) {
      if (!exists || requested.has(path)) continue
      requested.add(path)
      queue.push(path)
    }
    drainQueue()
  })

  onMount(() => {
    void loadActivityMap()
    // Leaving Home abandons probes that have not started.
    return () => (queue.length = 0)
  })

  async function rescan(): Promise<void> {
    try {
      await refreshActivityMap()
    } catch (error) {
      notify.error('Rescan activity failed', getErrorMessage(error))
    }
  }
</script>

{#snippet heading(label: string)}
  <h2 class="m-0 text-xs tracking-widest text-muted uppercase">{label}</h2>
{/snippet}

{#snippet continueRow(workbench: WorkbenchInfo, server?: string)}
  <ContinueRow
    {workbench}
    highlighted={highlightPath !== null && workbench.folders.includes(highlightPath)}
    closing={closing?.has(`${server ?? ''}:${workbench.id}`) ?? false}
    error={closeErrors?.[`${server ?? ''}:${workbench.id}`]}
    onOpen={server ? () => onOpenWorkbench?.(server, workbench) : undefined}
    onTakeOver={() => onTakeOverWorkbench(workbench, server)}
    onClose={() => onCloseWorkbench?.(workbench, server)}
  />
{/snippet}
<StageView>
  <div class="flex flex-col gap-8">
    <BlurFade delay={0.05} duration={0.5} direction="up" offset={10}>
      <header class="flex items-center gap-3">
        <img src={appIconUrl} alt="" class="size-7.5 shrink-0" />
        <div class="min-w-0 flex-1">
          <h1 class="m-0 text-2xl font-medium text-bright">Sworm</h1>
          <p class="m-0 text-base text-muted">Agentic Development Environment</p>
        </div>
        {#if actions}{@render actions()}{/if}
      </header>
    </BlurFade>

    {#if hasContinue}
      <BlurFade delay={0.12} duration={0.4} direction="up" offset={8}>
        <section class="flex flex-col gap-2">
          {@render heading('Continue')}
          {#each sections as { server, workbenches } (server ?? '')}
            {#if workbenches.length > 0}
              {#if server}<h3 class="m-0 pt-2 font-mono text-xs text-muted">{server}</h3>{/if}
              <ul class="-mx-3 my-0 flex list-none flex-col gap-0.5 p-0">
                {#each workbenches as workbench (workbench.id)}
                  {@render continueRow(workbench, server)}
                {/each}
              </ul>
            {/if}
          {/each}
        </section>
      </BlurFade>
    {/if}

    <BlurFade delay={0.2} duration={0.4} direction="up" offset={8}>
      <section class="flex flex-col gap-3">
        <div class="flex items-center gap-2">
          {@render heading('Projects')}
          <InfoTooltip ariaLabel="What are Projects?">
            <p class="max-w-60 text-sm leading-snug">
              Folders opened in Sworm, plus folders where coding agents ran on this machine. Agent history is scanned
              locally; nothing leaves the device.
            </p>
          </InfoTooltip>
          <IconButton tooltip="Rescan agent history" onclick={rescan} disabled={scanning}>
            <RefreshCw size={11} class={scanning ? 'animate-spin' : ''} />
          </IconButton>
        </div>

        {#if projects.length === 0}
          <p class="m-0 text-base text-muted">
            {scanning ? 'Scanning agent history…' : 'No projects yet. Open a folder or run a coding agent in one.'}
          </p>
        {:else}
          <div class="grid grid-cols-2 gap-2.5 sm:grid-cols-3">
            {#each visible as project (project.path)}
              <ProjectCard
                {project}
                git={briefs.get(project.path)}
                openCount={openCounts.get(project.path) ?? 0}
                onOpen={() => onOpenProject(project.path)}
                onHighlight={(active) => {
                  if (active) highlightPath = project.path
                  else if (highlightPath === project.path) highlightPath = null
                }}
              />
            {/each}
          </div>
          {#if projects.length > COLLAPSED_PROJECTS}
            <Button variant="ghost" size="sm" class="self-start" onclick={() => (expanded = !expanded)}>
              {expanded ? 'Show Fewer' : `Show All ${projects.length}`}
            </Button>
          {/if}
        {/if}
      </section>
    </BlurFade>
  </div>
</StageView>
