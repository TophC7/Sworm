<script lang="ts">
  import { Command } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    heading,
    children,
    ...rest
  }: {
    class?: string
    heading?: string
    children?: Snippet
    value?: string
    forceMount?: boolean
  } = $props()
</script>

<Command.Group class={cn('overflow-hidden p-1', className)} {...rest}>
  {#if heading}
    <Command.GroupHeading class="px-2 py-1.5 text-xs font-medium text-muted">
      {heading}
    </Command.GroupHeading>
  {/if}
  <Command.GroupItems>
    {#if children}{@render children()}{/if}
  </Command.GroupItems>
</Command.Group>
