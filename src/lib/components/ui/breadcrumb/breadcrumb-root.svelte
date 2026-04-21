<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { HTMLAttributes } from 'svelte/elements'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    children,
    ...rest
  }: HTMLAttributes<HTMLElement> & {
    class?: string
    children?: Snippet
  } = $props()
</script>

<nav aria-label="Breadcrumb" class={cn('min-w-0', className)} {...rest}>
  {#if children}{@render children()}{/if}
</nav>
