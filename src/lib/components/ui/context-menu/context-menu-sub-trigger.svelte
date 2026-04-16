<script lang="ts">
  import { ContextMenu } from 'bits-ui'
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

<ContextMenu.SubTrigger
  class={cn(
    'flex w-full cursor-pointer items-center gap-2 rounded-sm px-3 py-1.5 text-left text-fg transition-colors outline-none hover:bg-surface focus:bg-surface data-[state=open]:bg-surface',
    className
  )}
  {...rest}
>
  {#if children}{@render children()}{/if}
</ContextMenu.SubTrigger>
