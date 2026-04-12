<script lang="ts">
	import type { Snippet } from 'svelte';
	import { setSidebarContext, type SidebarSide, type CollapsibleMode } from './sidebar-state.svelte';

	let {
		open = $bindable(true),
		side = 'left',
		collapsible = 'icon',
		onOpenChange,
		children
	}: {
		open?: boolean;
		side?: SidebarSide;
		collapsible?: CollapsibleMode;
		onOpenChange?: (open: boolean) => void;
		children?: Snippet;
	} = $props();

	function toggle() {
		open = !open;
		onOpenChange?.(open);
	}

	function setOpen(value: boolean) {
		open = value;
		onOpenChange?.(value);
	}

	setSidebarContext({
		get open() { return open; },
		get side() { return side; },
		get collapsible() { return collapsible; },
		toggle,
		setOpen
	});
</script>

<div
	class="flex h-full"
	data-sidebar-side={side}
	data-sidebar-state={open ? 'expanded' : 'collapsed'}
	data-sidebar-collapsible={collapsible}
>
	{#if children}{@render children()}{/if}
</div>
