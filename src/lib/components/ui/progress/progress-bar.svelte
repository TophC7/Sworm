<!--
  @component
  ProgressBar. Segmented mini progress fill on `bg-edge`. Shows a
  `done` segment (success) and an optional `active` segment (accent)
  layered against the total length. Empty state renders the bare
  track with no fill.
-->

<script lang="ts">
  import { cn } from '$lib/utils/cn'

  let {
    done,
    active = 0,
    total,
    class: className
  }: {
    done: number
    active?: number
    total: number
    class?: string
  } = $props()

  let pctDone = $derived(total > 0 ? Math.round((done / total) * 100) : 0)
  let pctActive = $derived(total > 0 ? Math.round((active / total) * 100) : 0)
  let resolvedTitle = $derived(
    total > 0 ? `${done} done, ${active} active, ${total - done - active} open` : 'No items'
  )
</script>

<div
  data-slot="progress-bar"
  class={cn('relative h-1 w-12 shrink-0 overflow-hidden rounded-sm bg-edge', className)}
  title={resolvedTitle}
  role="progressbar"
  aria-valuemin={0}
  aria-valuemax={total}
  aria-valuenow={done + active}
>
  {#if total > 0}
    <div class="absolute inset-y-0 left-0 bg-success" style:width="{pctDone}%"></div>
    <div class="absolute inset-y-0 bg-accent" style:left="{pctDone}%" style:width="{pctActive}%"></div>
  {/if}
</div>
