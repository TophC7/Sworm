<script lang="ts">
  import { DropdownMenu } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import { menuItemClass } from '../menu-recipe'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    destructive = false,
    disabled = false,
    onclick,
    children,
    ...rest
  }: {
    class?: string
    destructive?: boolean
    disabled?: boolean
    onclick?: (e?: Event) => void
    children?: Snippet
  } = $props()
</script>

<DropdownMenu.Item
  {disabled}
  onSelect={(e) => onclick?.(e)}
  class={cn(menuItemClass(destructive, disabled), className)}
  {...rest}
>
  {#if children}{@render children()}{/if}
</DropdownMenu.Item>
