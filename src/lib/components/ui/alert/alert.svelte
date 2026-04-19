<script lang="ts" module>
  import { tv, type VariantProps } from 'tailwind-variants'

  export const alertVariants = tv({
    base: 'relative flex min-w-0 items-start gap-2.5 rounded-lg border px-3 py-2.5 text-sm',
    variants: {
      variant: {
        info: 'border-edge bg-surface text-fg [&>svg]:text-muted',
        success: 'border-success/40 bg-success-bg text-success-bright [&>svg]:text-success',
        warning: 'border-warning/40 bg-warning-bg text-warning-bright [&>svg]:text-warning',
        error: 'border-danger-border bg-danger-bg text-danger-bright [&>svg]:text-danger'
      }
    },
    defaultVariants: {
      variant: 'info'
    }
  })

  export type AlertVariant = VariantProps<typeof alertVariants>['variant']
</script>

<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    variant = 'info',
    class: className,
    children,
    ...rest
  }: {
    variant?: AlertVariant
    class?: string
    children?: Snippet
    [key: string]: unknown
  } = $props()
</script>

<div role="alert" class={cn(alertVariants({ variant }), className)} {...rest}>
  {#if children}{@render children()}{/if}
</div>
