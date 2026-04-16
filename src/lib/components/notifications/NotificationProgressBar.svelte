<script lang="ts">
  import TabBeam, { type BeamVariant } from '$lib/components/ui/tab-beam.svelte'
  import { cn } from '$lib/utils/cn'

  let {
    progress,
    variant = 'accent' as BeamVariant,
    class: className
  }: {
    progress?: number
    variant?: BeamVariant
    class?: string
  } = $props()

  let isDeterminate = $derived(progress != null)
  let width = $derived(`${Math.min(100, Math.max(0, progress ?? 100))}%`)
</script>

<div class={cn('relative h-1.5 overflow-hidden rounded-full bg-ground/70', className)} aria-hidden="true">
  {#if isDeterminate}
    <div class="absolute inset-y-0 left-0 overflow-hidden rounded-full" style={`width: ${width};`}>
      <div class="relative h-full">
        <TabBeam {variant} class="h-full rounded-full" />
      </div>
    </div>
  {:else}
    <TabBeam {variant} class="h-full rounded-full" />
  {/if}
</div>
