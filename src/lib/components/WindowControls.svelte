<script lang="ts">
	import { getWindowControls } from '$lib/stores/ui.svelte'
	import { Maximize } from '@lucide/svelte'
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
	<div class="flex items-center shrink-0 -mr-0.5">
		{#if config.showMinimize}
			<button
				class="flex items-center justify-center w-11 h-9 bg-transparent border-none text-muted hover:text-fg hover:bg-raised transition-colors cursor-pointer"
				onclick={() => appWindow.minimize()}
				title="Minimize"
			>
				<Minus size={14} strokeWidth={1.5} />
			</button>
		{/if}

		{#if config.showMaximize}
			<button
				class="flex items-center justify-center w-11 h-9 bg-transparent border-none text-muted hover:text-fg hover:bg-raised transition-colors cursor-pointer"
				onclick={() => appWindow.toggleMaximize()}
				title={maximized ? 'Restore' : 'Maximize'}
			>
				{#if maximized}
					<!-- Restore icon: two overlapping squares -->
					<svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.2">
						<rect x="3.5" y="4.5" width="7" height="7" rx="1" />
						<path d="M5.5 4.5V3.5a1 1 0 011-1h4a1 1 0 011 1v4a1 1 0 01-1 1h-1" />
					</svg>
				{:else}
					<Maximize size={12} strokeWidth={1.5} />
				{/if}
			</button>
		{/if}

		{#if config.showClose}
			<button
				class="flex items-center justify-center w-11 h-9 bg-transparent border-none text-muted hover:text-bright hover:bg-danger transition-colors cursor-pointer"
				onclick={() => appWindow.close()}
				title="Close"
			>
				<X size={14} strokeWidth={1.5} />
			</button>
		{/if}
	</div>
{/if}
