<script lang="ts">
  import { DropdownMenu as DropdownMenuPrimitive } from 'bits-ui'
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
  }: WithoutChildrenOrChild<DropdownMenuPrimitive.CheckboxItemProps> & {
    children?: Snippet
  } = $props()
</script>

<DropdownMenuPrimitive.CheckboxItem
  bind:ref
  bind:checked
  data-slot="dropdown-menu-checkbox-item"
  class={cn(menuIndicatorItemClass, className)}
  {...rest}
>
  {#snippet children({ checked })}
    <span class={menuIndicatorClass}>
      {#if checked}<Check size={14} class="text-current" />{/if}
    </span>
    {@render label?.()}
  {/snippet}
</DropdownMenuPrimitive.CheckboxItem>
