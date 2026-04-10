<script lang="ts">
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import { loadSettings, getSettings, getSettingsLoading, saveGeneralSettings, saveProviderConfig } from '$lib/stores/settings.svelte';
	import { refreshProviders } from '$lib/stores/providers.svelte';

	let settings = $derived(getSettings());
	let loading = $derived(getSettingsLoading());

	let generalDraft = $state({
		theme: 'system',
		terminal_font_family: 'JetBrains Mono',
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

		savingProviders = {
			...savingProviders,
			[providerId]: true
		};
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
			savingProviders = {
				...savingProviders,
				[providerId]: false
			};
		}
	}

	function updateProviderDraft(
		providerId: string,
		key: 'enabled' | 'binaryPath' | 'extraArgs',
		value: boolean | string
	) {
		providerDrafts = {
			...providerDrafts,
			[providerId]: {
				...providerDrafts[providerId],
				[key]: value
			}
		};
	}
</script>

<div class="settings-page">
	<header class="settings-header">
		<div>
			<p class="eyebrow">Settings</p>
			<h1>App Configuration</h1>
			<p class="header-copy">Provider detection, launch overrides, and baseline desktop preferences.</p>
		</div>

		<button class="back-btn" onclick={() => goto('/')}>Back to Workspace</button>
	</header>

	{#if flashMessage}
		<div class="flash">{flashMessage}</div>
	{/if}

	{#if loading && !settings}
		<div class="empty-card">Loading settings…</div>
	{:else if settings}
		<div class="settings-grid">
			<section class="settings-card">
				<div class="card-header">
					<div>
						<h2>General</h2>
						<p>Minimal desktop preferences persisted in `app_settings`.</p>
					</div>
					<button class="save-btn" onclick={handleSaveGeneral} disabled={savingGeneral}>
						{savingGeneral ? 'Saving…' : 'Save'}
					</button>
				</div>

				<label class="field">
					<span>Theme</span>
					<select bind:value={generalDraft.theme}>
						<option value="system">System</option>
						<option value="dark">Dark</option>
						<option value="light">Light</option>
					</select>
				</label>

				<label class="field">
					<span>Terminal font family</span>
					<input bind:value={generalDraft.terminal_font_family} />
				</label>

				<label class="field">
					<span>Terminal font size</span>
					<input type="number" min="10" max="24" bind:value={generalDraft.terminal_font_size} />
				</label>
			</section>

			<section class="settings-card provider-section">
				<div class="card-header">
					<div>
						<h2>Providers</h2>
						<p>Detection results and launch overrides for Claude Code and Codex.</p>
					</div>
				</div>

				<div class="provider-list">
					{#each settings.providers as entry (entry.provider.id)}
						<div class="provider-card">
							<div class="provider-topline">
								<div>
									<h3>{entry.provider.label}</h3>
									<p class="provider-meta">
										{entry.provider.version ?? 'Version unavailable'}
										{#if entry.provider.resolved_path}
											· {entry.provider.resolved_path}
										{/if}
									</p>
								</div>
								<span class="status-badge {entry.provider.status}">{entry.provider.status}</span>
							</div>

							{#if entry.provider.message}
								<p class="provider-message">{entry.provider.message}</p>
							{/if}

							{#if entry.provider.status === 'missing'}
								<p class="install-hint">{entry.provider.install_hint}</p>
							{/if}

							<label class="field inline">
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
								<span>Enabled</span>
							</label>

							<label class="field">
								<span>Binary path override</span>
								<input
									value={providerDrafts[entry.provider.id]?.binaryPath ?? ''}
									oninput={(event) =>
										updateProviderDraft(
											entry.provider.id,
											'binaryPath',
											(event.currentTarget as HTMLInputElement).value
										)}
									placeholder="Use detected CLI when empty"
								/>
							</label>

							<label class="field">
								<span>Extra args</span>
								<input
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

							<div class="provider-actions">
								<button
									class="save-btn"
									onclick={() => handleSaveProvider(entry.provider.id)}
									disabled={savingProviders[entry.provider.id]}
								>
									{savingProviders[entry.provider.id] ? 'Saving…' : 'Save Provider'}
								</button>
							</div>
						</div>
					{/each}
				</div>
			</section>
		</div>
	{:else}
		<div class="empty-card">No settings data available.</div>
	{/if}
</div>

<style>
	.settings-page {
		height: 100vh;
		overflow-y: auto;
		padding: 24px;
		background:
			radial-gradient(circle at top left, rgba(88, 166, 255, 0.12), transparent 32%),
			linear-gradient(180deg, #0d1117 0%, #11161d 100%);
	}

	.settings-header {
		display: flex;
		align-items: flex-start;
		justify-content: space-between;
		gap: 24px;
		margin-bottom: 20px;
	}

	.eyebrow {
		margin: 0 0 6px;
		font-size: 0.75rem;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: #8b949e;
	}

	h1,
	h2,
	h3,
	p {
		margin: 0;
	}

	.header-copy {
		margin-top: 8px;
		max-width: 640px;
		color: #8b949e;
	}

	.back-btn,
	.save-btn {
		border: 1px solid #30363d;
		background: #161b22;
		color: #f0f6fc;
		padding: 10px 14px;
		border-radius: 10px;
		cursor: pointer;
	}

	.back-btn:hover,
	.save-btn:hover {
		border-color: #58a6ff;
	}

	.back-btn:disabled,
	.save-btn:disabled {
		opacity: 0.6;
		cursor: default;
	}

	.flash,
	.empty-card,
	.settings-card,
	.provider-card {
		border: 1px solid #30363d;
		background: rgba(22, 27, 34, 0.92);
		box-shadow: 0 20px 60px rgba(0, 0, 0, 0.2);
	}

	.flash,
	.empty-card {
		padding: 14px 16px;
		border-radius: 14px;
		margin-bottom: 20px;
	}

	.flash {
		color: #3fb950;
	}

	.settings-grid {
		display: grid;
		grid-template-columns: minmax(280px, 360px) minmax(0, 1fr);
		gap: 20px;
	}

	.settings-card {
		border-radius: 18px;
		padding: 20px;
	}

	.card-header {
		display: flex;
		align-items: flex-start;
		justify-content: space-between;
		gap: 16px;
		margin-bottom: 18px;
	}

	.card-header p {
		margin-top: 6px;
		color: #8b949e;
	}

	.field {
		display: flex;
		flex-direction: column;
		gap: 8px;
		margin-bottom: 16px;
		color: #c9d1d9;
		font-size: 0.9rem;
	}

	.field.inline {
		flex-direction: row;
		align-items: center;
	}

	.field input,
	.field select {
		width: 100%;
		padding: 10px 12px;
		border-radius: 10px;
		border: 1px solid #30363d;
		background: #0d1117;
		color: #f0f6fc;
	}

	.provider-list {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
		gap: 16px;
	}

	.provider-card {
		border-radius: 14px;
		padding: 16px;
	}

	.provider-topline {
		display: flex;
		align-items: flex-start;
		justify-content: space-between;
		gap: 16px;
		margin-bottom: 14px;
	}

	.provider-meta,
	.provider-message,
	.install-hint {
		color: #8b949e;
		font-size: 0.84rem;
	}

	.provider-message,
	.install-hint {
		margin-bottom: 14px;
	}

	.status-badge {
		border-radius: 999px;
		padding: 4px 10px;
		font-size: 0.72rem;
		text-transform: uppercase;
		letter-spacing: 0.06em;
	}

	.status-badge.connected {
		background: rgba(63, 185, 80, 0.14);
		color: #3fb950;
	}

	.status-badge.missing {
		background: rgba(210, 153, 34, 0.14);
		color: #d29922;
	}

	.status-badge.error {
		background: rgba(248, 81, 73, 0.14);
		color: #f85149;
	}

	.provider-actions {
		display: flex;
		justify-content: flex-end;
		margin-top: 12px;
	}

	@media (max-width: 960px) {
		.settings-page {
			padding: 16px;
		}

		.settings-header,
		.card-header {
			flex-direction: column;
		}

		.settings-grid {
			grid-template-columns: 1fr;
		}
	}
</style>
