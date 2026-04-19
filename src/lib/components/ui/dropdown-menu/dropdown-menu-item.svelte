<script lang="ts">
  import { DropdownMenu } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    destructive = false,
    disabled = false,
    onclick,
    children,
    ...rest
  }: {
    class?: string
    destructive?: boolean
    disabled?: boolean
    onclick?: (e?: Event) => void
    children?: Snippet
  } = $props()
</script>

<DropdownMenu.Item
  {disabled}
  onSelect={(e) => onclick?.(e)}
  class={cn(
    'w-full rounded-sm px-3 py-1.5 text-left transition-colors outline-none focus-visible:shadow-focus-ring',
    disabled
      ? 'cursor-default text-subtle'
      : destructive
        ? 'cursor-pointer text-danger hover:bg-danger-bg focus:bg-danger-bg'
        : 'cursor-pointer text-fg hover:bg-surface focus:bg-surface',
    className
  )}
  {...rest}
>
  {#if children}{@render children()}{/if}
</DropdownMenu.Item>
