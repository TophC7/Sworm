<script lang="ts">
	import { Menubar } from 'bits-ui';
	import { cn } from '$lib/utils/cn';
	import type { Snippet } from 'svelte';

	let {
		class: className,
		disabled = false,
		onclick,
		children,
		...rest
	}: {
		class?: string;
		disabled?: boolean;
		onclick?: (e?: Event) => void;
		children?: Snippet;
	} = $props();
</script>

<Menubar.Item
	{disabled}
	onSelect={(e) => onclick?.(e)}
	class={cn(
		'w-full px-3 py-1.5 text-left cursor-pointer rounded-sm transition-colors outline-none flex items-center justify-between gap-4',
		disabled
			? 'text-muted/50 cursor-default'
			: 'text-fg hover:bg-surface focus:bg-surface',
		className
	)}
	{...rest}
>
	{#if children}{@render children()}{/if}
</Menubar.Item>
