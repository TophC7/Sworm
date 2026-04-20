<!--
  @component
  Switch — bits-ui wrapper with Sworm design tokens.

  A binary on/off control styled as a soft toggle. Use where
  the action is an immediate state flip (enable/disable) rather
  than a form field being ticked.
-->

<script lang="ts">
  import { Switch } from 'bits-ui'
  import { cn } from '$lib/utils/cn'

  let {
    checked = $bindable(false),
    disabled = false,
    class: className,
    onCheckedChange,
    ...rest
  }: {
    checked?: boolean
    disabled?: boolean
    class?: string
    onCheckedChange?: (v: boolean) => void
    [key: string]: unknown
  } = $props()
</script>

<Switch.Root
  data-slot="switch"
  bind:checked
  {disabled}
  {onCheckedChange}
  class={cn(
    'relative inline-flex h-[18px] w-[32px] shrink-0 cursor-pointer items-center rounded-full border border-edge bg-surface transition-colors',
    'hover:border-accent',
    'data-[state=checked]:border-accent data-[state=checked]:bg-accent',
    'focus-visible:shadow-focus-ring focus-visible:outline-none',
    'disabled:pointer-events-none disabled:opacity-50',
    className
  )}
  {...rest}
>
  <Switch.Thumb
    class={cn(
      'pointer-events-none block h-[12px] w-[12px] translate-x-[2px] rounded-full bg-bright shadow-sm transition-transform',
      'data-[state=checked]:translate-x-[16px] data-[state=checked]:bg-ground'
    )}
  />
</Switch.Root>
