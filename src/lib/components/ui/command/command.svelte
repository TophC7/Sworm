<script lang="ts">
  import { Command } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    children,
    ...rest
  }: {
    class?: string
    children?: Snippet
    value?: string
    onValueChange?: (value: string) => void
    shouldFilter?: boolean
    loop?: boolean
    label?: string
    vimBindings?: boolean
  } = $props()
</script>

<Command.Root class={cn('flex h-full w-full flex-col overflow-hidden rounded-xl bg-raised', className)} {...rest}>
  {#if children}{@render children()}{/if}
</Command.Root>
