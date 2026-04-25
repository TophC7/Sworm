<script lang="ts">
  import { Command } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    value = $bindable(''),
    children,
    ...rest
  }: {
    class?: string
    value?: string
    children?: Snippet
    onValueChange?: (value: string) => void
    shouldFilter?: boolean
    loop?: boolean
    label?: string
    vimBindings?: boolean
    disablePointerSelection?: boolean
    onkeydown?: (event: KeyboardEvent) => void
    onpointermovecapture?: (event: PointerEvent) => void
    onpointerleavecapture?: (event: PointerEvent) => void
  } = $props()
</script>

<Command.Root
  bind:value
  class={cn('flex h-full w-full flex-col overflow-hidden rounded-xl bg-raised', className)}
  {...rest}
>
  {#if children}{@render children()}{/if}
</Command.Root>
