<script lang="ts">
  import { PaneGroup as PaneGroupPrimitive, type PaneGroupProps } from 'paneforge'
  import { cn } from '$lib/utils/cn'

  let { direction = 'horizontal', class: className, children, ...rest }: PaneGroupProps & { class?: string } = $props()
</script>

<PaneGroupPrimitive
  {direction}
  class={cn('flex h-full w-full', direction === 'vertical' ? 'flex-col' : '', className)}
  {...rest}
>
  {#if children}{@render children()}{/if}
</PaneGroupPrimitive>
