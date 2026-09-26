<script lang="ts">
  import { Particles } from '$lib/components/ui/particles'
  import type { Snippet } from 'svelte'

  let {
    children,
    // Mirrors --color-accent (src/app.css). Particles' canvas needs a
    // literal hex; kept in sync with the accent token.
    particleColor = '#ffb59f',
    particleQuantity = 75
  }: {
    children: Snippet
    particleColor?: string
    particleQuantity?: number
  } = $props()
</script>

<!-- Content centers while it fits and scrolls once it outgrows the stage. -->
<div class="relative flex flex-1 flex-col overflow-y-auto bg-ground">
  <Particles
    class="z-0 opacity-60"
    quantity={particleQuantity}
    color={particleColor}
    size={0.5}
    staticity={40}
    ease={60}
  />

  <div class="relative z-10 m-auto w-full max-w-2xl px-8 py-10">
    {@render children()}
  </div>
</div>
