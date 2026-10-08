<!--
  @component
  ToggleChip. Small pressed/unpressed pill used for filter chips
  (P0..P5 priorities, status filters, etc.). Sets aria-pressed.
-->

<script lang="ts" module>
  import { tv, type VariantProps } from 'tailwind-variants'

  export const toggleChipVariants = tv({
    base: 'inline-flex h-6 min-w-6 items-center justify-center rounded-md border px-1.5 font-mono text-2xs transition-colors focus-visible:shadow-focus-ring focus-visible:outline-none disabled:pointer-events-none disabled:opacity-50',
    variants: {
      pressed: {
        true: 'border-accent bg-accent-dim text-ground',
        false: 'border-edge text-muted hover:border-accent hover:text-bright'
      }
    },
    defaultVariants: {
      pressed: false
    }
  })

  export type ToggleChipPressed = VariantProps<typeof toggleChipVariants>['pressed']
</script>

<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { HTMLButtonAttributes } from 'svelte/elements'
  import type { Snippet } from 'svelte'

  let {
    pressed = false,
    class: className,
    children,
    ...rest
  }: HTMLButtonAttributes & {
    pressed?: boolean
    class?: string
    children?: Snippet
  } = $props()
</script>

<button
  type="button"
  data-slot="toggle-chip"
  aria-pressed={pressed}
  class={cn(toggleChipVariants({ pressed }), className)}
  {...rest}
>
  {#if children}{@render children()}{/if}
</button>
