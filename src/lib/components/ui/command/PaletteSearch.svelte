<script lang="ts" module>
  import { tv } from 'tailwind-variants'

  export const paletteSearchInputVariants = tv({
    base: 'flex h-10 min-w-0 w-full bg-transparent py-3 text-sm text-fg outline-none placeholder:text-muted disabled:cursor-not-allowed disabled:opacity-50'
  })
</script>

<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import { SearchIcon } from '$lib/icons/lucideExports'
  import type { Snippet } from 'svelte'
  import type { HTMLAttributes } from 'svelte/elements'

  let {
    class: className,
    children,
    ...rest
  }: Omit<HTMLAttributes<HTMLDivElement>, 'children'> & {
    children?: Snippet<[inputClass: string]>
  } = $props()
</script>

<div
  data-slot="palette-search"
  class={cn('flex shrink-0 items-center gap-2 border-b border-edge px-3 focus-within:border-accent', className)}
  {...rest}
>
  <SearchIcon class="size-4 shrink-0 text-muted" aria-hidden="true" />
  {#if children}{@render children(paletteSearchInputVariants())}{/if}
</div>
