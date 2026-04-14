<script lang="ts">
  import { Command } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    children,
    ...rest
  }: {
    class?: string
    children?: Snippet
  } = $props()
</script>

<Command.Empty class={cn('py-6 text-center text-sm text-muted', className)} {...rest}>
  {#if children}
    {@render children()}
  {:else}
    No results found.
  {/if}
</Command.Empty>
