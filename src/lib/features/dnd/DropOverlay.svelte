<script lang="ts">
  import { onMount, tick } from 'svelte'
  import { zoneGeometry, type Zone } from '$lib/features/dnd/overlay'

  let {
    visible = false,
    zone = 'merge' as Zone,
    label = ''
  }: {
    visible?: boolean
    zone?: Zone
    label?: string
  } = $props()

  let canAnimate = $state(false)
  let geometry = $derived(zoneGeometry(zone))

  onMount(() => {
    void tick().then(() => {
      canAnimate = true
    })
  })
</script>

{#if visible}
  <div class="pointer-events-none absolute inset-0 z-30 p-2">
    <div
      class="pointer-events-none absolute rounded-xl border border-accent/60 bg-accent/20 shadow-[inset_0_0_0_1px_rgba(255,181,159,0.16)] {zone ===
      'merge'
        ? 'ring-1 ring-accent/30'
        : ''} {canAnimate ? 'transition-[top,left,width,height,opacity] duration-75 ease-out' : ''}"
      style="top: {geometry.top}; left: {geometry.left}; width: {geometry.width}; height: {geometry.height};"
    >
      {#if label}
        <div class="pointer-events-none absolute inset-0 flex items-center justify-center">
          <span
            class="rounded border border-edge-strong/70 bg-raised/85 px-2 py-0.5 text-xs font-medium tracking-wide text-bright uppercase"
          >
            {label}
          </span>
        </div>
      {/if}
    </div>
  </div>
{/if}
