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

<ContextMenu.Portal>
  <ContextMenu.Content
    class={cn('z-50 min-w-[180px] rounded-lg border border-edge bg-raised py-1 text-base shadow-popover', className)}
    {...rest}
  >
    {#if children}{@render children()}{/if}
  </ContextMenu.Content>
</ContextMenu.Portal>
