<script lang="ts" module>
  import { tv, type VariantProps } from 'tailwind-variants'

  export const alertVariants = tv({
    base: 'relative flex min-w-0 items-start gap-2.5 border',
    variants: {
      variant: {
        info: 'border-edge bg-surface text-fg [&>svg]:text-muted',
        success: 'border-success/40 bg-success-bg text-success-bright [&>svg]:text-success',
        warning: 'border-warning/40 bg-warning-bg text-warning-bright [&>svg]:text-warning',
        danger: 'border-danger-border bg-danger-bg text-danger-bright [&>svg]:text-danger'
      },
      layout: {
        card: 'rounded-lg px-3 py-2.5 text-sm',
        banner: 'rounded-none border-x-0 border-t-0 px-2.5 py-1.5 text-xs'
      }
    },
    defaultVariants: {
      variant: 'info',
      layout: 'card'
    }
  })

  export type AlertVariant = VariantProps<typeof alertVariants>['variant']
  export type AlertLayout = VariantProps<typeof alertVariants>['layout']
</script>

<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    variant = 'info',
    layout = 'card',
    class: className,
    children,
    ...rest
  }: {
    variant?: AlertVariant
    layout?: AlertLayout
    class?: string
    children?: Snippet
    [key: string]: unknown
  } = $props()
</script>

<div role="alert" class={cn(alertVariants({ variant, layout }), className)} {...rest}>
  {#if children}{@render children()}{/if}
</div>
