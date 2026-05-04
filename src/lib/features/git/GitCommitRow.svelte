<!--
  @component
  GitCommitRow: reusable commit row with graph or compact branch-history chrome.

  @param commit - commit metadata shown in the row and tooltip.
  @param detail - lazily-loaded commit detail used for tooltip stats.
  @param graphColor - lane or branch color used for node and ref badges.
  @param render - optional graph render data. Omit for compact history rows.
  @param showRefs - whether ref badges appear inline after the message.
-->

<script lang="ts">
  import type { CommitDetail, GraphCommit } from '$lib/types/backend'
  import type { RowRender } from '$lib/features/git/graph'
  import { CIRCLE_RADIUS, SWIMLANE_HEIGHT } from '$lib/features/git/graph'
  import CommitTooltip from '$lib/features/git/CommitTooltip.svelte'
  import { refLabel, visibleRefs } from '$lib/features/git/gitRefs'
  import { TooltipContent, TooltipRoot, TooltipTrigger } from '$lib/components/ui/tooltip'

  let {
    commit,
    detail = null,
    graphColor,
    render = null,
    active = false,
    showRefs = true,
    indent = 0,
    onRowClick,
    onPrefetch
  }: {
    commit: GraphCommit
    detail?: CommitDetail | null
    graphColor: string
    render?: RowRender | null
    active?: boolean
    showRefs?: boolean
    indent?: number
    onRowClick?: (hash: string) => void
    onPrefetch?: (hash: string) => void
  } = $props()

  let refs = $derived(visibleRefs(commit.refs))
</script>

<TooltipRoot>
  <TooltipTrigger
    type="button"
    class="group flex w-full items-center border-0 border-t border-edge/30 bg-transparent p-0 text-left hover:bg-raised focus-visible:shadow-focus-ring focus-visible:outline-none {active
      ? 'bg-raised'
      : ''}"
    style="height: {SWIMLANE_HEIGHT}px; padding-left: {indent}px"
    aria-label={commit.message}
    onclick={() => onRowClick?.(commit.hash)}
    onmouseenter={() => onPrefetch?.(commit.hash)}
    onfocus={() => onPrefetch?.(commit.hash)}
  >
    {#if render}
      <svg
        class="shrink-0"
        width={render.width}
        height={SWIMLANE_HEIGHT}
        viewBox="0 0 {render.width} {SWIMLANE_HEIGHT}"
      >
        {#each render.paths as p, pi (pi)}
          <path d={p.d} stroke={p.color} fill="none" stroke-width="1" stroke-linecap="round" />
        {/each}
        {#if render.circle.isMerge}
          <circle
            cx={render.circle.cx}
            cy={render.circle.cy}
            r={CIRCLE_RADIUS + 2}
            fill={render.circle.color}
            stroke="none"
          />
          <circle
            cx={render.circle.cx}
            cy={render.circle.cy}
            r={CIRCLE_RADIUS - 1}
            fill={render.circle.color}
            stroke="none"
          />
        {:else}
          <circle
            cx={render.circle.cx}
            cy={render.circle.cy}
            r={render.circle.r}
            fill={render.circle.color}
            stroke="none"
          />
        {/if}
      </svg>
    {:else}
      <span class="sr-only">Commit</span>
    {/if}

    <div class="flex min-w-0 flex-1 items-center gap-1.5 pr-2">
      <span class="min-w-0 truncate text-sm text-fg">{commit.message}</span>
      {#if showRefs && refs.length > 0}
        <span
          class="ml-auto inline-flex max-w-24 shrink-0 items-center truncate rounded px-1 py-px font-mono text-2xs leading-tight"
          style="background: color-mix(in srgb, {graphColor} 20%, transparent); color: {graphColor}"
        >
          {refLabel(refs[0])}
        </span>
        {#each refs.slice(1) as ref (ref)}
          <span
            class="size-3 shrink-0 rounded-sm"
            style="background: color-mix(in srgb, {graphColor} 40%, transparent)"
          ></span>
        {/each}
      {/if}
    </div>
  </TooltipTrigger>
  <TooltipContent class="max-w-135" sideOffset={6} side="right" align="center">
    <CommitTooltip {commit} {detail} {graphColor} />
  </TooltipContent>
</TooltipRoot>
