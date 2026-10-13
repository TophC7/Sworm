<script lang="ts">
  import { cn, type WithElementRef } from '$lib/utils/cn'
  import { menuShortcutClass } from '../menu-recipe'
  import type { HTMLAttributes } from 'svelte/elements'

  let {
    ref = $bindable(null),
    class: className,
    children,
    ...rest
  }: WithElementRef<HTMLAttributes<HTMLSpanElement>> = $props()
</script>

<span bind:this={ref} data-slot="context-menu-shortcut" class={cn(menuShortcutClass, className)} {...rest}>
  {#if children}{@render children()}{/if}
</span>
