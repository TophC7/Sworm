<script lang="ts">
  import { DropdownMenu } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    children,
    sideOffset = 4,
    ...rest
  }: {
    class?: string
    children?: Snippet
    sideOffset?: number
  } = $props()
</script>

<DropdownMenu.Portal>
  <DropdownMenu.Content
    {sideOffset}
    class={cn(
      'z-50 min-w-[180px] rounded-lg border border-edge bg-raised py-1 text-[0.8rem] shadow-[0_8px_24px_rgba(0,0,0,0.4)]',
      className
    )}
    {...rest}
  >
    {#if children}{@render children()}{/if}
  </DropdownMenu.Content>
</DropdownMenu.Portal>
