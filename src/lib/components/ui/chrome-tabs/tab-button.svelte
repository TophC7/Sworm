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

  export const tabButtonVariants = tv({
    base: 'group relative flex items-center gap-1.5 px-3 shrink-0 border-none cursor-pointer transition-colors',
    variants: {
      variant: {
        project: 'text-[0.78rem] rounded-t h-8',
        pane: 'text-[0.75rem] self-stretch'
      },
      active: {
        true: '',
        false: 'bg-transparent text-muted hover:text-fg hover:bg-surface/50'
      }
    },
    compoundVariants: [
      { variant: 'project', active: true, class: 'bg-ground text-bright' },
      { variant: 'pane', active: true, class: 'bg-surface text-fg' }
    ],
    defaultVariants: {
      variant: 'pane',
      active: false
    }
  })

  export type TabButtonVariant = VariantProps<typeof tabButtonVariants>['variant']
</script>

<script lang="ts">
  import TabBeam, { type BeamVariant } from '$lib/components/ui/tab-beam.svelte'
  import { cn } from '$lib/utils/cn'
  import X from '@lucide/svelte/icons/x'
  import type { Snippet } from 'svelte'
  import type { HTMLButtonAttributes } from 'svelte/elements'

  type CloseEvent = MouseEvent | KeyboardEvent

  let {
    variant = 'pane',
    active = false,
    showBeam,
    beamVariant = 'accent' as BeamVariant,
    leading,
    onClose,
    class: className,
    children,
    ...rest
  }: HTMLButtonAttributes & {
    variant?: TabButtonVariant
    active?: boolean
    showBeam?: boolean
    beamVariant?: BeamVariant
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
  {#if active && effectiveShowBeam}<TabBeam variant={beamVariant} />{/if}
  {#if leading}{@render leading()}{/if}
  {#if children}{@render children()}{/if}
  {#if onClose}
    <span
      class="text-[0.7rem] leading-none text-muted opacity-0 transition-all group-hover:opacity-100 hover:text-danger"
      role="button"
      tabindex="0"
      onclick={handleClose}
      onkeydown={(event) => event.key === 'Enter' && handleClose(event)}
    >
      <X size={12} />
    </span>
  {/if}
</button>
