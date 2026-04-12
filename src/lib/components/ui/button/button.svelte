<script lang="ts" module>
  import { tv, type VariantProps } from 'tailwind-variants'

  export const buttonVariants = tv({
    base: 'inline-flex items-center justify-center gap-1.5 rounded-lg font-medium cursor-pointer transition-colors disabled:pointer-events-none disabled:opacity-50',
    variants: {
      variant: {
        default: 'bg-raised border border-edge text-fg hover:border-accent hover:text-bright',
        ghost: 'bg-transparent border-none text-muted hover:text-bright hover:bg-surface',
        outline: 'bg-transparent border border-edge text-fg hover:border-accent hover:text-bright',
        destructive: 'bg-danger-bg border border-danger-border text-danger hover:text-danger-bright',
        accent: 'bg-accent-dim border border-accent-dim text-ground hover:bg-accent'
      },
      size: {
        default: 'px-3.5 py-2 text-[0.82rem]',
        sm: 'px-2.5 py-1 text-[0.72rem]',
        xs: 'px-2 py-0.5 text-[0.68rem]',
        icon: 'w-7 h-7 p-0',
        'icon-sm': 'w-5 h-5 p-0'
      }
    },
    defaultVariants: {
      variant: 'default',
      size: 'default'
    }
  })

  export type ButtonVariant = VariantProps<typeof buttonVariants>['variant']
  export type ButtonSize = VariantProps<typeof buttonVariants>['size']
</script>

<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { HTMLButtonAttributes } from 'svelte/elements'
  import type { Snippet } from 'svelte'

  let {
    variant = 'default',
    size = 'default',
    class: className,
    children,
    ...rest
  }: HTMLButtonAttributes & {
    variant?: ButtonVariant
    size?: ButtonSize
    class?: string
    children?: Snippet
  } = $props()
</script>

<button class={cn(buttonVariants({ variant, size }), className)} {...rest}>
  {#if children}{@render children()}{/if}
</button>
