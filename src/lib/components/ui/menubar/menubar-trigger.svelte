<script lang="ts">
  import { Menubar } from 'bits-ui'
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

<Menubar.Trigger
  class={cn(
    'cursor-pointer rounded px-2 py-1 text-[0.75rem] text-muted transition-colors select-none',
    'hover:bg-raised/60 hover:text-fg',
    'data-[state=open]:bg-raised/80 data-[state=open]:text-fg',
    'outline-none focus-visible:ring-1 focus-visible:ring-accent/40',
    className
  )}
  {...rest}
>
  {#if children}{@render children()}{/if}
</Menubar.Trigger>
