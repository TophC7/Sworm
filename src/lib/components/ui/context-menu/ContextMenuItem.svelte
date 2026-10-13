<script lang="ts">
  import { ContextMenu as ContextMenuPrimitive } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import { menuItemClass } from '../menu-recipe'

  let {
    ref = $bindable(null),
    class: className,
    inset,
    destructive = false,
    disabled = false,
    onclick,
    ...rest
  }: ContextMenuPrimitive.ItemProps & {
    inset?: boolean
    destructive?: boolean
  } = $props()

  // bits turns Enter/Space into a DOM click, so `onclick` also covers keyboard activation.
  // bits only guards its own select handler; disabled rows keep pointer events for
  // cursor-not-allowed, so the consumer's click is dropped here.
</script>

<ContextMenuPrimitive.Item
  bind:ref
  data-slot="context-menu-item"
  data-inset={inset || undefined}
  data-destructive={destructive || undefined}
  {disabled}
  onclick={disabled ? undefined : onclick}
  class={cn(menuItemClass, className)}
  {...rest}
/>
