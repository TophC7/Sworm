<script lang="ts">
  import { DropdownMenu } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    destructive = false,
    onclick,
    children,
    ...rest
  }: {
    class?: string
    destructive?: boolean
    onclick?: (e?: Event) => void
    children?: Snippet
  } = $props()
</script>

<DropdownMenu.Item
  onSelect={(e) => onclick?.(e)}
  class={cn(
    'w-full cursor-pointer rounded-sm px-3 py-1.5 text-left transition-colors outline-none',
    destructive ? 'text-danger hover:bg-danger-bg focus:bg-danger-bg' : 'text-fg hover:bg-surface focus:bg-surface',
    className
  )}
  {...rest}
>
  {#if children}{@render children()}{/if}
</DropdownMenu.Item>
