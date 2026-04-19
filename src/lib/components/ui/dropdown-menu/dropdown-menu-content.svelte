<script lang="ts">
  import { DropdownMenu } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    children,
    sideOffset = 4,
    align,
    ...rest
  }: {
    class?: string
    children?: Snippet
    sideOffset?: number
    align?: 'start' | 'center' | 'end'
  } = $props()
</script>

<DropdownMenu.Portal>
  <DropdownMenu.Content
    {sideOffset}
    {align}
    class={cn('z-50 min-w-[180px] rounded-lg border border-edge bg-raised py-1 text-base shadow-popover', className)}
    {...rest}
  >
    {#if children}{@render children()}{/if}
  </DropdownMenu.Content>
</DropdownMenu.Portal>
