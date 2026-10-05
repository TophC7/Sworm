<script lang="ts">
  import { Particles } from '$lib/components/ui/particles'
  import type { Snippet } from 'svelte'

  let {
    children,
    footer
  }: {
    children: Snippet
    /** Sits at the stage bottom while content fits, after the content once it scrolls. */
    footer?: Snippet
  } = $props()
</script>

<!-- Content centers while it fits and scrolls once it outgrows the stage. -->
<div class="relative flex flex-1 flex-col overflow-y-auto bg-ground">
  <Particles class="z-0 opacity-60" />

  <div class="relative z-10 m-auto w-full max-w-2xl px-8 py-10">
    {@render children()}
  </div>

  {#if footer}
    <div class="relative z-10 flex shrink-0 justify-center px-8 pb-6">{@render footer()}</div>
  {/if}
</div>
