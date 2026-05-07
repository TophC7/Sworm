<!--
  @component
  ProgressBar. Segmented mini progress fill on `bg-edge`. Shows a
  `done` segment (success) and an optional `active` segment (accent)
  layered against the total length. Empty state renders the bare
  track with no fill.
-->

<script lang="ts" module>
  import { tv, type VariantProps } from 'tailwind-variants'

  export const progressBarVariants = tv({
    base: 'relative shrink-0 overflow-hidden rounded-sm bg-edge',
    variants: {
      size: {
        sm: 'h-1 w-12',
        md: 'h-1.5 w-20'
      }
    },
    defaultVariants: {
      size: 'sm'
    }
  })

  export type ProgressBarSize = VariantProps<typeof progressBarVariants>['size']
</script>

<script lang="ts">
  import { cn } from '$lib/utils/cn'

  let {
    done,
    active = 0,
    total,
    size = 'sm' as ProgressBarSize,
    class: className,
    title
  }: {
    done: number
    active?: number
    total: number
    size?: ProgressBarSize
    class?: string
    title?: string
  } = $props()

  let pctDone = $derived(total > 0 ? Math.round((done / total) * 100) : 0)
  let pctActive = $derived(total > 0 ? Math.round((active / total) * 100) : 0)
  let resolvedTitle = $derived(
    title ?? (total > 0 ? `${done} done, ${active} active, ${total - done - active} open` : 'No items')
  )
</script>

<div
  data-slot="progress-bar"
  class={cn(progressBarVariants({ size }), className)}
  title={resolvedTitle}
  role="progressbar"
  aria-valuemin={0}
  aria-valuemax={total}
  aria-valuenow={done + active}
>
  {#if total > 0}
    <div class="absolute inset-y-0 left-0 bg-success" style:width="{pctDone}%"></div>
    <div class="absolute inset-y-0 bg-accent" style:left="{pctDone}%" style:width="{pctActive}%"></div>
  {/if}
</div>
