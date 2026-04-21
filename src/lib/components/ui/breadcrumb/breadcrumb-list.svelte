<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { HTMLAttributes } from 'svelte/elements'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    children,
    ...rest
  }: HTMLAttributes<HTMLOListElement> & {
    class?: string
    children?: Snippet
  } = $props()
</script>

<ol class={cn('flex min-w-0 items-center gap-1.5 text-sm text-muted', className)} {...rest}>
  {#if children}{@render children()}{/if}
</ol>
