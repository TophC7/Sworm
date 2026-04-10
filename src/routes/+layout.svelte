<script lang="ts">
	import { onMount } from 'svelte';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { disposeAll } from '$lib/terminal/sessionRegistry';
	import type { Snippet } from 'svelte';

	let { children }: { children: Snippet } = $props();

	onMount(() => {
		const unlisten = getCurrentWindow().onCloseRequested(() => {
			disposeAll();
		});

		return () => {
			unlisten.then((cleanup) => cleanup()).catch(() => {});
		};
	});
</script>

<div class="app-root">
	{@render children()}
</div>

<style>
	:global(body) {
		margin: 0;
		padding: 0;
		font-family: system-ui, -apple-system, 'Segoe UI', sans-serif;
		background: #0d1117;
		color: #c9d1d9;
		overflow: hidden;
	}

	:global(*) {
		box-sizing: border-box;
	}

	:global(a) {
		color: #58a6ff;
		text-decoration: none;
	}

	:global(a:hover) {
		text-decoration: underline;
	}

	:global(::-webkit-scrollbar) {
		width: 6px;
	}

	:global(::-webkit-scrollbar-track) {
		background: transparent;
	}

	:global(::-webkit-scrollbar-thumb) {
		background: #30363d;
		border-radius: 3px;
	}

	.app-root {
		height: 100vh;
		display: flex;
		flex-direction: column;
	}
</style>
