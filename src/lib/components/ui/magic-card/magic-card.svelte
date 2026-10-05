<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    children,
    class: className,
    gradientFrom = 'var(--color-accent)',
    gradientTo = 'var(--color-warm)',
    disabled = false,
    onclick
  }: {
    children?: Snippet
    class?: string
    gradientFrom?: string
    gradientTo?: string
    disabled?: boolean
    onclick?: () => void
  } = $props()

  const OFF_SCREEN = -200
  let mouseX = $state(OFF_SCREEN)
  let mouseY = $state(OFF_SCREEN)

  function reset() {
    mouseX = OFF_SCREEN
    mouseY = OFF_SCREEN
  }

  function handlePointerMove(e: PointerEvent) {
    if (disabled) return
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
    mouseX = e.clientX - rect.left
    mouseY = e.clientY - rect.top
  }

  let borderBg = $derived(
    `radial-gradient(200px circle at ${mouseX}px ${mouseY}px, ${gradientFrom}, ${gradientTo}, var(--color-edge) 100%)`
  )
  let overlayBg = $derived(
    `radial-gradient(200px circle at ${mouseX}px ${mouseY}px, var(--color-raised), transparent 100%)`
  )
</script>

<!-- The button's own fill is the resting 1px border; the inner layer covers all but that line. -->
<button
  class={cn(
    'group relative rounded-lg bg-edge text-left focus-visible:shadow-focus-ring focus-visible:outline-none',
    disabled ? 'cursor-default opacity-40 grayscale' : 'cursor-pointer',
    className
  )}
  type="button"
  {disabled}
  onpointermove={handlePointerMove}
  onpointerleave={reset}
  onclick={() => onclick?.()}
>
  <!-- Border gradient: lights the 1px edge near the cursor -->
  <div
    class="pointer-events-none absolute inset-0 rounded-[inherit] opacity-0 transition-opacity duration-300 group-hover:opacity-100"
    style="background: {borderBg};"
    aria-hidden="true"
  ></div>

  <!-- Inner background -->
  <div class="absolute inset-px rounded-[inherit] bg-surface" aria-hidden="true"></div>

  <!-- Spotlight on hover: lifts the fill toward raised around the cursor -->
  <div
    class="pointer-events-none absolute inset-px rounded-[inherit] transition-opacity duration-300"
    style="background: {overlayBg}; opacity: {mouseX > 0 ? 0.8 : 0};"
    aria-hidden="true"
  ></div>

  <!-- Content -->
  <div class="relative">
    {#if children}{@render children()}{/if}
  </div>
</button>
