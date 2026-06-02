<!--
  @component
  MenubarTrigger — a top-level menu button (File / View / …) in the
  titlebar menu bar. Flat to match the surrounding chrome; highlights on
  hover and while its menu is open.
-->

<script lang="ts">
  import { Menubar } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let { class: className, children, ...rest }: { class?: string; children?: Snippet } = $props()
</script>

<Menubar.Trigger
  class={cn(
    'flex cursor-pointer items-center rounded-sm px-2 py-1 text-sm text-muted transition-colors select-none',
    'hover:bg-raised hover:text-bright',
    'focus-visible:shadow-focus-ring focus-visible:outline-none',
    'data-[state=open]:bg-raised data-[state=open]:text-bright',
    className
  )}
  {...rest}
>
  {#if children}{@render children()}{/if}
</Menubar.Trigger>
