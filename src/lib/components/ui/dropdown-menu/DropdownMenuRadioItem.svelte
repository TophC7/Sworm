<script lang="ts">
  import { DropdownMenu as DropdownMenuPrimitive } from 'bits-ui'
  import { cn, type WithoutChild } from '$lib/utils/cn'
  import { Circle } from '$lib/icons/lucideExports'
  import { menuIndicatorClass, menuIndicatorItemClass } from '../menu-recipe'

  let {
    ref = $bindable(null),
    class: className,
    children: label,
    ...rest
  }: WithoutChild<DropdownMenuPrimitive.RadioItemProps> = $props()
</script>

<DropdownMenuPrimitive.RadioItem
  bind:ref
  data-slot="dropdown-menu-radio-item"
  class={cn(menuIndicatorItemClass, className)}
  {...rest}
>
  {#snippet children({ checked })}
    <span class={menuIndicatorClass}>
      {#if checked}<Circle class="size-2 fill-current text-current" />{/if}
    </span>
    {@render label?.({ checked })}
  {/snippet}
</DropdownMenuPrimitive.RadioItem>
