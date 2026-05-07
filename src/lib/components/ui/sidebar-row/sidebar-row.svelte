<!--
  @component
  SidebarRow. Single source of truth for sidebar list rows: epic and
  branch group headers, branch / stash leaves, "show more" actions,
  and non-interactive info rows. The class recipe is exported from the
  module script for cases that need a different wrapping element (e.g.
  a tooltip trigger or context-menu host) while still pinning the
  visual contract to one place.

  Variants:
    - section: h-7, hover:bg-raised. Group/section header.
    - leaf:    h-6, hover:bg-raised. Tree row for an item.
    - action:  h-6, text-xs muted, hover:bg-raised. "Show more" / inline "+ Add".
    - info:    h-6, text-xs muted, no hover. Loading / empty / dir placeholder.

  `divider` toggles the `border-t border-edge/30` rule independently
  from variant; `depth` resolves via treeIndent for nested tree rows;
  `pressed` applies the bg-raised accent for current/expanded items.
-->

<script lang="ts" module>
  import { tv, type VariantProps } from 'tailwind-variants'

  export const sidebarRowVariants = tv({
    base: 'flex w-full items-center gap-1.5 px-2.5 text-left transition-colors focus-visible:shadow-focus-ring focus-visible:outline-none',
    variants: {
      variant: {
        section: 'h-7 text-sm hover:bg-raised',
        leaf: 'h-6 text-sm hover:bg-raised',
        action: 'h-6 text-sm text-muted hover:bg-raised hover:text-bright',
        info: 'h-6 text-xs text-muted'
      },
      divider: {
        true: 'border-t border-edge/30',
        false: ''
      },
      pressed: {
        true: 'bg-raised',
        false: ''
      }
    },
    defaultVariants: {
      variant: 'section',
      divider: true,
      pressed: false
    }
  })

  export type SidebarRowVariant = NonNullable<VariantProps<typeof sidebarRowVariants>['variant']>
</script>

<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import { treeIndent } from '$lib/components/ui/tree-indent'
  import type { Snippet } from 'svelte'

  let {
    variant = 'section' as SidebarRowVariant,
    pressed = false,
    divider = true,
    depth = 0,
    class: className,
    children,
    ...rest
  }: {
    variant?: SidebarRowVariant
    pressed?: boolean
    divider?: boolean
    depth?: number
    class?: string
    children: Snippet
    [key: string]: unknown
  } = $props()

  let interactive = $derived(variant !== 'info')
  let style = $derived(depth > 0 ? `padding-left: ${treeIndent(depth)}` : undefined)
</script>

{#if interactive}
  <button
    type="button"
    data-slot="sidebar-row"
    class={cn(sidebarRowVariants({ variant, pressed, divider }), className)}
    {style}
    {...rest}
  >
    {@render children()}
  </button>
{:else}
  <div
    data-slot="sidebar-row"
    class={cn(sidebarRowVariants({ variant, pressed, divider }), className)}
    {style}
    {...rest}
  >
    {@render children()}
  </div>
{/if}
