<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { HTMLAnchorAttributes } from 'svelte/elements'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    children,
    ...rest
  }: HTMLAnchorAttributes & {
    class?: string
    children?: Snippet
  } = $props()
</script>

<a class={cn('truncate text-muted transition-colors hover:text-bright', className)} {...rest}>
  {#if children}{@render children()}{/if}
</a>
