<script lang="ts">
  import { Tabs } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    value,
    children,
    ...rest
  }: {
    class?: string
    value: string
    children?: Snippet
  } = $props()
</script>

<Tabs.Trigger
  {value}
  class={cn(
    'cursor-pointer px-2.5 py-1 text-[0.72rem] transition-colors',
    'data-[state=active]:bg-accent-bg data-[state=active]:text-accent',
    'data-[state=inactive]:text-muted data-[state=inactive]:hover:text-fg',
    className
  )}
  {...rest}
>
  {#if children}{@render children()}{/if}
</Tabs.Trigger>
