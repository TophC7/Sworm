<script lang="ts">
	import { getWindowControls } from '$lib/stores/ui.svelte'
	import { Maximize, Minimize } from '@lucide/svelte'
	import Minus from '@lucide/svelte/icons/minus'
	import X from '@lucide/svelte/icons/x'
	import { getCurrentWindow } from '@tauri-apps/api/window'

	let config = $derived(getWindowControls());
	let maximized = $state(false);

	const appWindow = getCurrentWindow();

	// Track maximized state for visual feedback
	$effect(() => {
		const unlisten = appWindow.onResized(async () => {
			maximized = await appWindow.isMaximized();
		});
		// Check initial state
		appWindow.isMaximized().then((v) => (maximized = v));
		return () => { unlisten.then((fn) => fn()); };
	});
</script>

{#if !config.useSystemDecorations}
	<div class="flex items-center shrink-0 pr-1">
		{#if config.showMinimize}
			<button
				class="flex items-center justify-center w-8 h-8 rounded-md border-none bg-transparent text-muted hover:text-fg hover:bg-raised/70 transition-colors cursor-pointer"
				onclick={() => appWindow.minimize()}
				title="Minimize"
			>
				<Minus size={12} strokeWidth={2} />
			</button>
		{/if}

		{#if config.showMaximize}
			<button
				class="flex items-center justify-center w-8 h-8 rounded-md border-none bg-transparent text-muted hover:text-fg hover:bg-raised/70 transition-colors cursor-pointer"
				onclick={() => appWindow.toggleMaximize()}
				title={maximized ? 'Restore' : 'Maximize'}
			>
				{#if maximized}
					<Minimize size={10} strokeWidth={2} />
				{:else}
					<Maximize size={10} strokeWidth={2} />
				{/if}
			</button>
		{/if}

		{#if config.showClose}
			<button
				class="flex items-center justify-center w-8 h-8 rounded-md border-none bg-transparent text-muted hover:text-bright hover:bg-danger-bg transition-colors cursor-pointer"
				onclick={() => appWindow.close()}
				title="Close"
			>
				<X size={11} strokeWidth={2} />
			</button>
		{/if}
	</div>
{/if}
