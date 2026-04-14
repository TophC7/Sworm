<script lang="ts">
  import { Command } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import SearchIcon from '@lucide/svelte/icons/search'

  let {
    class: className,
    value = $bindable(''),
    ...rest
  }: {
    class?: string
    value?: string
    placeholder?: string
  } = $props()
</script>

<div class="flex items-center gap-2 border-b border-edge px-3" data-slot="command-input-wrapper">
  <SearchIcon class="size-4 shrink-0 text-muted" />
  <Command.Input
    class={cn(
      'flex h-10 w-full bg-transparent py-3 text-sm text-fg outline-none',
      'placeholder:text-muted disabled:cursor-not-allowed disabled:opacity-50',
      className
    )}
    bind:value
    {...rest}
  />
</div>
