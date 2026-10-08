<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let { class: className, children }: { class?: string; children?: Snippet } = $props()
</script>

<div class={cn('mb-2.5', className)}>
  {#if children}{@render children()}{/if}
</div>
