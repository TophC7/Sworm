<script lang="ts" module>
  import { tv, type VariantProps } from 'tailwind-variants'

  export const buttonGroupVariants = tv({
    base: 'flex w-fit items-stretch [&>*]:focus-visible:relative [&>*]:focus-visible:z-10',
    variants: {
      orientation: {
        horizontal:
          '[&>[data-slot]:not(:has(~[data-slot]))]:rounded-r-md! [&>[data-slot]]:rounded-r-none [&>[data-slot]~[data-slot]]:rounded-l-none [&>[data-slot]~[data-slot]]:border-l-0',
        vertical:
          'flex-col [&>[data-slot]:not(:has(~[data-slot]))]:rounded-b-md! [&>[data-slot]]:rounded-b-none [&>[data-slot]~[data-slot]]:rounded-t-none [&>[data-slot]~[data-slot]]:border-t-0'
      }
    },
    defaultVariants: {
      orientation: 'horizontal'
    }
  })

  export type ButtonGroupOrientation = VariantProps<typeof buttonGroupVariants>['orientation']
</script>

<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { HTMLAttributes } from 'svelte/elements'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    children,
    orientation = 'horizontal',
    ...rest
  }: HTMLAttributes<HTMLDivElement> & {
    orientation?: ButtonGroupOrientation
    children?: Snippet
  } = $props()
</script>

<div
  role="group"
  data-slot="button-group"
  data-orientation={orientation}
  class={cn(buttonGroupVariants({ orientation }), className)}
  {...rest}
>
  {#if children}{@render children()}{/if}
</div>
