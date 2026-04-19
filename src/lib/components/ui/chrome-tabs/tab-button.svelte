<!--
  @component
  TabButton -- shared app tab chrome with active styling, beam, and close affordance

  @param variant - visual variant for project or pane tab buttons
  @param active - whether this tab is currently selected
  @param leading - optional snippet rendered before the main label content
  @param onClose - optional close handler that enables the close affordance
-->

<script lang="ts" module>
  import { tv, type VariantProps } from 'tailwind-variants'

  // Project vs pane share 95% of the chrome. The only real differences:
  //   1. The animated beam sits at the bottom for project, top for pane (handled in the template).
  //   2. Active/hover bg steps up one surface from whichever parent the strip lives on —
  //      project lives on `surface` (titlebar) → steps to `raised`.
  //      pane    lives on `ground`   (pane header) → steps to `surface`.
  // Everything else (height-fills-container, padding, text role, focus ring, close affordance)
  // is identical. Keep the `variant` prop as a data hook so we can diverge later without
  // rewriting consumers.
  export const tabButtonVariants = tv({
    base:
      'group relative flex h-full items-center gap-1.5 px-3 shrink-0 text-sm border-none cursor-pointer transition-colors ' +
      'focus-visible:shadow-focus-ring focus-visible:outline-none',
    variants: {
      variant: {
        project: '',
        pane: ''
      },
      active: {
        true: 'text-bright',
        false: 'bg-transparent text-muted'
      }
    },
    compoundVariants: [
      { variant: 'project', active: true, class: 'bg-raised' },
      { variant: 'project', active: false, class: 'hover:bg-raised/50 hover:text-bright' },
      { variant: 'pane', active: true, class: 'bg-surface' },
      { variant: 'pane', active: false, class: 'hover:bg-surface/50 hover:text-bright' }
    ],
    defaultVariants: {
      variant: 'pane',
      active: false
    }
  })

  export type TabButtonVariant = VariantProps<typeof tabButtonVariants>['variant']
</script>

<script lang="ts">
  import TabBeam from '$lib/components/ui/tab-beam.svelte'
  import { cn } from '$lib/utils/cn'
  import { X } from '$lib/icons/lucideExports'
  import type { Snippet } from 'svelte'
  import type { HTMLButtonAttributes } from 'svelte/elements'

  type CloseEvent = MouseEvent | KeyboardEvent

  let {
    variant = 'pane',
    active = false,
    showBeam,
    leading,
    onClose,
    class: className,
    children,
    ...rest
  }: HTMLButtonAttributes & {
    variant?: TabButtonVariant
    active?: boolean
    showBeam?: boolean
    leading?: Snippet
    onClose?: (event: CloseEvent) => void | Promise<void>
    class?: string
    children?: Snippet
  } = $props()

  // Derive from active when not explicitly provided.
  // Cannot use a $props() default for this — defaults capture the
  // initial value of `active`, not a reactive binding.
  let effectiveShowBeam = $derived(showBeam ?? active)

  function handleClose(event: CloseEvent) {
    event.stopPropagation()
    if (!onClose) return
    void onClose(event)
  }
</script>

<button class={cn(tabButtonVariants({ variant, active }), className)} role="tab" aria-selected={active} {...rest}>
  {#if active && effectiveShowBeam}
    <TabBeam />
  {/if}
  {#if leading}{@render leading()}{/if}
  {#if children}{@render children()}{/if}
  {#if onClose}
    <span
      class=" -mr-1 p-0.5 text-xs leading-none text-muted opacity-0 transition-all group-hover:opacity-100 hover:text-danger"
      role="button"
      tabindex="0"
      onclick={handleClose}
      onkeydown={(event) => event.key === 'Enter' && handleClose(event)}
    >
      <X size={12} />
    </span>
  {/if}
</button>
