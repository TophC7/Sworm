<script lang="ts">
  import { ContextMenu } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import { menuContentClass } from '../menu-recipe'
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
    class={cn(menuContentClass, className)}
    {...rest}
  >
    {#if children}{@render children()}{/if}
  </ContextMenu.Content>
</ContextMenu.Portal>
