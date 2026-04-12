<!--
  @component
  TooltipContent -- styled tooltip content wrapper for ADE

  @param class - optional extra classes for the floating content
  @param sideOffset - gap between trigger and tooltip content
-->

<script lang="ts">
	import { Tooltip } from 'bits-ui';
	import { cn } from '$lib/utils/cn';
	import type { Snippet } from 'svelte';

	let {
		class: className,
		children,
		sideOffset = 8,
		...rest
	}: {
		class?: string;
		children?: Snippet;
		sideOffset?: number;
	} = $props();
</script>

<Tooltip.Portal>
	<Tooltip.Content
		{sideOffset}
		class={cn(
			'z-50 max-w-80 rounded-lg border border-edge bg-overlay px-3 py-2 text-[0.72rem] leading-5 text-fg shadow-[0_10px_30px_rgba(0,0,0,0.45)]',
			className
		)}
		{...rest}
	>
		{#if children}{@render children()}{/if}
	</Tooltip.Content>
</Tooltip.Portal>
