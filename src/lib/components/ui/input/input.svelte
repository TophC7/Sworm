<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { HTMLInputAttributes } from 'svelte/elements'

  let {
    class: className,
    value = $bindable(''),
    ref = $bindable<HTMLInputElement | null>(null),
    ...rest
  }: Omit<HTMLInputAttributes, 'value'> & {
    class?: string
    value?: string | number
    ref?: HTMLInputElement | null
  } = $props()
</script>

<input
  data-slot="input"
  bind:value
  bind:this={ref}
  class={cn(
    'w-full rounded border border-edge bg-surface px-2.5 py-1.5 text-base text-fg transition-colors outline-none placeholder:text-subtle focus:border-accent',
    className
  )}
  {...rest}
/>
