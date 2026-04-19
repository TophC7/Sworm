<script lang="ts">
  import { ContextMenu } from 'bits-ui'
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

<ContextMenu.Item
  {disabled}
  onSelect={(e) => onclick?.(e)}
  class={cn(
    'flex w-full items-center gap-2 rounded-sm px-3 py-1.5 text-left transition-colors outline-none focus-visible:shadow-focus-ring',
    disabled
      ? 'cursor-not-allowed text-muted/50'
      : destructive
        ? 'cursor-pointer text-danger hover:bg-danger-bg focus:bg-danger-bg'
        : 'cursor-pointer text-fg hover:bg-surface focus:bg-surface',
    className
  )}
  {...rest}
>
  {#if children}{@render children()}{/if}
</ContextMenu.Item>
