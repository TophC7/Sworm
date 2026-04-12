<script lang="ts">
  import { Tabs } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let { class: className, children, ...rest }: { class?: string; children?: Snippet } = $props()
</script>

<Tabs.List
  class={cn('inline-flex items-center overflow-hidden rounded border border-edge bg-ground', className)}
  {...rest}
>
  {#if children}{@render children()}{/if}
</Tabs.List>
