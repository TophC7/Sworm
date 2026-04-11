<script lang="ts">
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import { loadSettings, getSettings, getSettingsLoading, saveGeneralSettings, saveProviderConfig } from '$lib/stores/settings.svelte';
	import { refreshProviders } from '$lib/stores/providers.svelte';

	let settings = $derived(getSettings());
	let loading = $derived(getSettingsLoading());

	let generalDraft = $state({
		theme: 'dark',
		terminal_font_family: 'Monocraft',
		terminal_font_size: 13
	});
	let providerDrafts = $state<Record<string, { enabled: boolean; binaryPath: string; extraArgs: string }>>({});
	let savingGeneral = $state(false);
	let savingProviders = $state<Record<string, boolean>>({});
	let flashMessage = $state<string | null>(null);

	onMount(() => {
		void loadSettings();
	});

	$effect(() => {
		if (!settings) {
			return;
		}

		generalDraft = { ...settings.general };
		providerDrafts = Object.fromEntries(
			settings.providers.map((entry) => [
				entry.provider.id,
				{
					enabled: entry.config.enabled,
					binaryPath: entry.config.binary_path_override ?? '',
					extraArgs: entry.config.extra_args.join(' ')
				}
			])
		);
	});

	async function handleSaveGeneral() {
		savingGeneral = true;
		flashMessage = null;
		try {
			await saveGeneralSettings({
				theme: generalDraft.theme,
				terminal_font_family: generalDraft.terminal_font_family,
				terminal_font_size: Number(generalDraft.terminal_font_size)
			});
			flashMessage = 'General settings saved.';
		} finally {
			savingGeneral = false;
		}
	}

	async function handleSaveProvider(providerId: string) {
		const draft = providerDrafts[providerId];
		if (!draft) {
			return;
		}

		savingProviders = { ...savingProviders, [providerId]: true };
		flashMessage = null;

		try {
			await saveProviderConfig({
				provider_id: providerId,
				enabled: draft.enabled,
				binary_path_override: draft.binaryPath.trim() || null,
				extra_args: draft.extraArgs
					.split(/\s+/)
					.map((value) => value.trim())
					.filter(Boolean)
			});
			await loadSettings();
			await refreshProviders();
			flashMessage = `Saved ${providerId} settings.`;
		} finally {
			savingProviders = { ...savingProviders, [providerId]: false };
		}
	}

	function updateProviderDraft(
		providerId: string,
		key: 'enabled' | 'binaryPath' | 'extraArgs',
		value: boolean | string
	) {
		providerDrafts = {
			...providerDrafts,
			[providerId]: { ...providerDrafts[providerId], [key]: value }
		};
	}

	function statusBadgeClass(status: string): string {
		if (status === 'connected') return 'bg-success/15 text-success';
		if (status === 'missing') return 'bg-warning/15 text-warning';
		if (status === 'error') return 'bg-danger/15 text-danger';
		return 'bg-edge text-muted';
	}
</script>

<div class="flex-1 flex flex-col overflow-hidden bg-ground">
	<header class="flex items-center justify-between px-3.5 py-2 bg-surface border-b border-edge min-h-10 gap-3 shrink-0">
		<div class="flex items-center gap-3">
			<button
				class="text-muted text-[0.82rem] hover:text-bright transition-colors cursor-pointer bg-transparent border-none"
				onclick={() => goto('/')}
			>&larr; Back</button>
			<span class="text-[0.72rem] text-edge-strong">|</span>
			<h1 class="m-0 text-[0.95rem] font-semibold text-bright">Settings</h1>
		</div>
	</header>

	{#if flashMessage}
		<div class="px-3.5 py-2 text-success text-[0.82rem] border-b border-edge bg-success/5">
			{flashMessage}
		</div>
	{/if}

	<div class="flex-1 overflow-y-auto">
		{#if loading && !settings}
			<div class="px-3.5 py-8 text-muted text-[0.82rem]">Loading settings&hellip;</div>
		{:else if settings}
			<div class="border-b border-edge">
				<div class="flex items-center justify-between px-3.5 py-2">
					<span class="text-[0.7rem] font-semibold uppercase tracking-wide text-muted">Terminal</span>
					<button
						class="btn-sm"
						onclick={handleSaveGeneral}
						disabled={savingGeneral}
					>
						{savingGeneral ? 'Saving\u2026' : 'Save'}
					</button>
				</div>

				<div class="px-3.5 pb-3 flex flex-col gap-3">
					<label class="flex items-center gap-3 text-[0.82rem]">
						<span class="text-muted w-28 shrink-0">Font family</span>
						<input
							class="field-input flex-1"
							bind:value={generalDraft.terminal_font_family}
						/>
					</label>
					<label class="flex items-center gap-3 text-[0.82rem]">
						<span class="text-muted w-28 shrink-0">Font size</span>
						<input
							class="field-input w-20"
							type="number"
							min="10"
							max="24"
							bind:value={generalDraft.terminal_font_size}
						/>
					</label>
				</div>
			</div>

			<div>
				<div class="px-3.5 py-2">
					<span class="text-[0.7rem] font-semibold uppercase tracking-wide text-muted">Providers</span>
				</div>

				{#each settings.providers as entry (entry.provider.id)}
					<div class="mx-3 mb-3 border border-edge rounded-lg overflow-hidden">
						<div class="flex items-center justify-between px-3 py-2 bg-surface">
							<div class="flex items-center gap-2 min-w-0">
								<span class="text-[0.85rem] font-semibold text-bright">{entry.provider.label}</span>
								<span class="rounded-full px-2 py-0.5 text-[0.68rem] uppercase tracking-wide {statusBadgeClass(entry.provider.status)}">
									{entry.provider.status}
								</span>
							</div>
							<button
								class="btn-sm"
								onclick={() => handleSaveProvider(entry.provider.id)}
								disabled={savingProviders[entry.provider.id]}
							>
								{savingProviders[entry.provider.id] ? 'Saving\u2026' : 'Save'}
							</button>
						</div>

						<div class="px-3 py-2.5 flex flex-col gap-2.5">
							<div class="text-[0.75rem] text-muted">
								{entry.provider.version ?? 'Version unavailable'}
								{#if entry.provider.resolved_path}
									<span class="text-subtle"> &middot; {entry.provider.resolved_path}</span>
								{/if}
							</div>

							{#if entry.provider.message}
								<div class="text-[0.75rem] text-warning">{entry.provider.message}</div>
							{/if}

							{#if entry.provider.status === 'missing'}
								<div class="text-[0.75rem] text-muted font-mono">{entry.provider.install_hint}</div>
							{/if}

							<label class="flex items-center gap-2 text-[0.82rem]">
								<input
									type="checkbox"
									checked={providerDrafts[entry.provider.id]?.enabled ?? true}
									onchange={(event) =>
										updateProviderDraft(
											entry.provider.id,
											'enabled',
											(event.currentTarget as HTMLInputElement).checked
										)}
								/>
								<span class="text-fg">Enabled</span>
							</label>

							<label class="flex items-center gap-3 text-[0.82rem]">
								<span class="text-muted w-28 shrink-0">Binary path</span>
								<input
									class="field-input flex-1"
									value={providerDrafts[entry.provider.id]?.binaryPath ?? ''}
									oninput={(event) =>
										updateProviderDraft(
											entry.provider.id,
											'binaryPath',
											(event.currentTarget as HTMLInputElement).value
										)}
									placeholder="Use detected CLI"
								/>
							</label>

							<label class="flex items-center gap-3 text-[0.82rem]">
								<span class="text-muted w-28 shrink-0">Extra args</span>
								<input
									class="field-input flex-1"
									value={providerDrafts[entry.provider.id]?.extraArgs ?? ''}
									oninput={(event) =>
										updateProviderDraft(
											entry.provider.id,
											'extraArgs',
											(event.currentTarget as HTMLInputElement).value
										)}
									placeholder="--flag value"
								/>
							</label>
						</div>
					</div>
				{/each}
			</div>
		{:else}
			<div class="px-3.5 py-8 text-muted text-[0.82rem]">No settings data available.</div>
		{/if}
	</div>
</div>
