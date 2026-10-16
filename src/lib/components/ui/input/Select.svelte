<!--
  @component
  Select: styled select control.
-->

<script lang="ts">
  import type { Snippet } from 'svelte'
  import type { HTMLSelectAttributes } from 'svelte/elements'
  import { cn } from '$lib/utils/cn'
  import { inputVariants } from './Input.svelte'

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

<select data-slot="select" bind:value class={cn(inputVariants(), 'scheme-dark', className)} {...rest}>
  {#if children}{@render children()}{/if}
</select>
