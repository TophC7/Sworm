<script lang="ts">
	import '../app.css';
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

<div class="flex flex-col h-screen">
	{@render children()}
</div>
