<!--
  @component
  ResizeDivider. 1px drag divider for resizing an adjacent panel. The
  visible line stays thin while a before: pseudo-element extends the
  grab area to a comfortable hit target. `onResizeStart` fires on grab
  so callers can cache layout reads once; `onResize` fires per pointer
  move with the live event and owns the size math.
-->

<script lang="ts" module>
  import { tv, type VariantProps } from 'tailwind-variants'

  export const resizeDividerVariants = tv({
    base: 'relative z-10 shrink-0 bg-edge transition-colors before:absolute hover:bg-accent/40',
    variants: {
      direction: {
        col: 'w-px cursor-col-resize before:inset-y-0 before:-inset-x-1.5',
        row: 'h-px cursor-row-resize before:inset-x-0 before:-inset-y-1.5'
      }
    },
    defaultVariants: {
      direction: 'col'
    }
  })

  export type ResizeDividerDirection = NonNullable<VariantProps<typeof resizeDividerVariants>['direction']>
</script>

<script lang="ts">
  import { cn } from '$lib/utils/cn'

  let {
    direction = 'col' as ResizeDividerDirection,
    onResizeStart,
    onResize,
    class: className,
    ...rest
  }: {
    direction?: ResizeDividerDirection
    onResizeStart?: (e: PointerEvent) => void
    onResize: (e: PointerEvent) => void
    class?: string
    [key: string]: unknown
  } = $props()

  // Window-level move/up listeners keep the drag alive once grabbed,
  // even when the pointer leaves the thin hit area.
  function dragHandle(element: HTMLElement) {
    function onPointerDown(e: PointerEvent) {
      e.preventDefault()
      onResizeStart?.(e)
      document.body.style.cursor = direction === 'col' ? 'col-resize' : 'row-resize'
      document.body.style.userSelect = 'none'

      function onMove(e: PointerEvent) {
        onResize(e)
      }
      function onUp() {
        document.body.style.cursor = ''
        document.body.style.userSelect = ''
        window.removeEventListener('pointermove', onMove)
        window.removeEventListener('pointerup', onUp)
      }
      window.addEventListener('pointermove', onMove)
      window.addEventListener('pointerup', onUp)
    }
    element.addEventListener('pointerdown', onPointerDown)
    return () => element.removeEventListener('pointerdown', onPointerDown)
  }
</script>

<div
  data-slot="resize-divider"
  class={cn(resizeDividerVariants({ direction }), className)}
  role="separator"
  aria-orientation={direction === 'col' ? 'vertical' : 'horizontal'}
  {@attach dragHandle}
  {...rest}
></div>
