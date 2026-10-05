<script lang="ts" module>
  import { tv } from 'tailwind-variants'

  export const commandItemVariants = tv({
    base: [
      'relative flex cursor-pointer items-center gap-2 rounded-lg px-2 py-1.5 text-sm text-fg outline-none select-none',
      'data-selected:bg-accent/15 data-selected:text-bright',
      'data-disabled:pointer-events-none data-disabled:opacity-50 disabled:pointer-events-none disabled:opacity-50',
      'focus-visible:shadow-focus-ring',
      '[&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-muted'
    ]
  })
</script>

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
    keywords?: string[]
    disabled?: boolean
    onSelect?: () => void
  } = $props()
</script>

<Command.Item class={cn(commandItemVariants(), className)} {...rest}>
  {#if children}{@render children()}{/if}
</Command.Item>
