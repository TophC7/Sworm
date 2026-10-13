<script lang="ts">
  import { ContextMenu as ContextMenuPrimitive } from 'bits-ui'
  import { cn, type WithoutChildrenOrChild } from '$lib/utils/cn'
  import { Check } from '$lib/icons/lucideExports'
  import { menuIndicatorClass, menuIndicatorItemClass } from '../menu-recipe'
  import type { Snippet } from 'svelte'

  let {
    ref = $bindable(null),
    checked = $bindable(false),
    class: className,
    children: label,
    ...rest
  }: WithoutChildrenOrChild<ContextMenuPrimitive.CheckboxItemProps> & {
    children?: Snippet
  } = $props()
</script>

<ContextMenuPrimitive.CheckboxItem
  bind:ref
  bind:checked
  data-slot="context-menu-checkbox-item"
  class={cn(menuIndicatorItemClass, className)}
  {...rest}
>
  {#snippet children({ checked })}
    <span class={menuIndicatorClass}>
      {#if checked}<Check size={14} class="text-current" />{/if}
    </span>
    {@render label?.()}
  {/snippet}
</ContextMenuPrimitive.CheckboxItem>
