<!--
  @component
  SearchInput: input with a leading search icon.
-->

<script lang="ts">
  import { SearchIcon } from '$lib/icons/lucideExports'
  import { cn } from '$lib/utils/cn'
  import Input from './input.svelte'
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

<div data-slot="search-input" class="relative">
  <SearchIcon
    class="pointer-events-none absolute left-2 top-1/2 size-3 -translate-y-1/2 text-subtle"
  />
  <Input bind:value bind:ref class={cn('pl-7', className)} {...rest} />
</div>
