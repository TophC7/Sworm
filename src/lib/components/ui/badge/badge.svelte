<script lang="ts" module>
	import { tv, type VariantProps } from 'tailwind-variants';

	export const badgeVariants = tv({
		base: 'inline-flex items-center rounded-full px-2 py-0.5 text-[0.68rem] uppercase tracking-wide font-medium',
		variants: {
			variant: {
				default: 'bg-edge text-muted',
				success: 'bg-success/15 text-success',
				warning: 'bg-warning/15 text-warning',
				danger: 'bg-danger/15 text-danger',
				accent: 'bg-accent-bg text-accent',
				muted: 'bg-edge text-muted'
			}
		},
		defaultVariants: {
			variant: 'default'
		}
	});

	export type BadgeVariant = VariantProps<typeof badgeVariants>['variant'];
</script>

<script lang="ts">
	import { cn } from '$lib/utils/cn';
	import type { HTMLAttributes } from 'svelte/elements';
	import type { Snippet } from 'svelte';

	let {
		variant = 'default',
		class: className,
		children,
		...rest
	}: HTMLAttributes<HTMLSpanElement> & {
		variant?: BadgeVariant;
		class?: string;
		children?: Snippet;
	} = $props();
</script>

<span class={cn(badgeVariants({ variant }), className)} {...rest}>
	{#if children}{@render children()}{/if}
</span>
