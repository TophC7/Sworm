<script lang="ts">
  import { DropdownMenu } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import { ChevronRight } from '$lib/icons/lucideExports'
  import type { Snippet } from 'svelte'

  let { class: className, children, ...rest }: { class?: string; children?: Snippet } = $props()
</script>

<DropdownMenu.SubTrigger
  data-slot="dropdown-menu-sub-trigger"
  class={cn(
    'flex w-full cursor-pointer items-center justify-between rounded-sm px-3 py-1.5 text-left text-fg transition-colors outline-none',
    'hover:bg-surface focus:bg-surface data-[state=open]:bg-surface',
    'focus-visible:shadow-focus-ring',
    className
  )}
  {...rest}
>
  {#if children}{@render children()}{/if}
  <ChevronRight size={12} class="ml-auto text-muted" />
</DropdownMenu.SubTrigger>
