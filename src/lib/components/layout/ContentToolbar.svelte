<script lang="ts">
  import type { Snippet } from 'svelte'

  let {
    left,
    right
  }: {
    left?: Snippet
    right?: Snippet
  } = $props()
</script>

<div class="flex shrink-0 items-center gap-3 border-b border-edge bg-surface/50 px-4 py-1.5 text-sm">
  {#if left}
    {@render left()}
  {/if}
  {#if right}
    <span class="ml-auto flex items-center gap-3">
      {@render right()}
    </span>
  {/if}
</div>
