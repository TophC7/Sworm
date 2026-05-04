<!--
  @component
  Select: styled select control.
-->

<script lang="ts">
  import type { Snippet } from 'svelte'
  import type { HTMLSelectAttributes } from 'svelte/elements'
  import { cn } from '$lib/utils/cn'

  let {
    class: className,
    value = $bindable(''),
    children,
    ...rest
  }: Omit<HTMLSelectAttributes, 'value'> & {
    class?: string
    value?: string
    children?: Snippet
  } = $props()
</script>

<select
  data-slot="select"
  bind:value
  class={cn(
    'w-full rounded border border-edge bg-surface px-2.5 py-1.5 text-base text-fg transition-colors outline-none focus:border-accent',
    className
  )}
  {...rest}
>
  {#if children}{@render children()}{/if}
</select>
