<!--
  @component
  StatusChip. Rounded status-bar control: `pill` for labelled chips
  (folder, branch, nix, remote), `circle` for icon-only ones (app info,
  notifications). Bits UI triggers take `statusChipVariants` directly.
-->

<script lang="ts" module>
  import { tv, type VariantProps } from 'tailwind-variants'

  export const statusChipVariants = tv({
    base: 'inline-flex h-5 shrink-0 cursor-pointer items-center rounded-full border bg-raised text-xs transition-colors focus-visible:shadow-focus-ring focus-visible:outline-none',
    variants: {
      shape: {
        pill: 'gap-1 px-2',
        circle: 'w-5 justify-center p-0'
      },
      tone: {
        default: 'border-edge text-muted hover:border-accent/50 hover:text-fg',
        success: 'border-success/40 bg-success/10 text-success hover:bg-success/20',
        warning: 'border-warning/40 bg-warning/10 text-warning-bright hover:bg-warning/20'
      },
      active: {
        true: 'border-accent/50 text-bright',
        false: ''
      }
    },
    defaultVariants: {
      shape: 'pill',
      tone: 'default',
      active: false
    }
  })

  export type StatusChipShape = VariantProps<typeof statusChipVariants>['shape']
  export type StatusChipTone = VariantProps<typeof statusChipVariants>['tone']
</script>

<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { HTMLButtonAttributes } from 'svelte/elements'
  import type { Snippet } from 'svelte'

  let {
    shape = 'pill',
    tone = 'default',
    active = false,
    class: className,
    children,
    ...rest
  }: HTMLButtonAttributes & {
    shape?: StatusChipShape
    tone?: StatusChipTone
    active?: boolean
    class?: string
    children?: Snippet
  } = $props()
</script>

<button
  type="button"
  data-slot="status-chip"
  class={cn(statusChipVariants({ shape, tone, active }), className)}
  {...rest}
>
  {#if children}{@render children()}{/if}
</button>
