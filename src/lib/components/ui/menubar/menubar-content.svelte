<script lang="ts">
	import { cn } from '$lib/utils/cn'
	import { Menubar } from 'bits-ui'
	import type { Snippet } from 'svelte'

	let {
		class: className,
		children,
		sideOffset = 4,
		alignOffset = -4,
		...rest
	}: {
		class?: string;
		children?: Snippet;
		sideOffset?: number;
		alignOffset?: number;
	} = $props();
</script>

<Menubar.Portal>
	<Menubar.Content
		{sideOffset}
		{alignOffset}
		class={cn(
			'z-50 min-w-45 bg-raised border border-edge rounded-lg shadow-[0_8px_24px_rgba(0,0,0,0.4)] py-1 text-[0.8rem]',
			className
		)}
		{...rest}
	>
		{#if children}{@render children()}{/if}
	</Menubar.Content>
</Menubar.Portal>
