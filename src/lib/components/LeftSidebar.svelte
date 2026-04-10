<script lang="ts">
	import { goto } from '$app/navigation';
	import { backend } from '$lib/api/backend';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import {
		getProjects,
		selectProject,
		getActiveProjectId,
		addProject,
		removeProject
	} from '$lib/stores/projects.svelte';
	import {
		getSessions,
		selectSession,
		getActiveSessionId,
		loadSessions,
		createSession,
		clearSessions,
		removeSession
	} from '$lib/stores/sessions.svelte';
	import { getProviders, getConnectedProviders } from '$lib/stores/providers.svelte';
	import type { ProviderStatus } from '$lib/types/backend';

	let projects = $derived(getProjects());
	let activeProjectId = $derived(getActiveProjectId());
	let sessions = $derived(getSessions());
	let activeSessionId = $derived(getActiveSessionId());
	let connectedProviders = $derived(getConnectedProviders());
	let showNewSession = $state(false);
	let dialogOpen = $state(false);
	let dialogTitle = $state('');
	let dialogMessage = $state('');
	let dialogConfirmLabel = $state('Confirm');
	let dialogShowCancel = $state(true);
	let dialogAction: (() => void | Promise<void>) | null = null;

	// Load sessions when active project changes
	$effect(() => {
		if (activeProjectId) {
			loadSessions(activeProjectId);
		} else {
			clearSessions();
		}
	});

	async function handleAddProject() {
		try {
			const path = await backend.projects.selectDirectory();
			if (path) {
				const project = await addProject(path);
				selectProject(project.id);
			}
		} catch (e) {
			openAlert('Project Error', `Failed to add project:\n${e}`);
		}
	}

	function handleSelectProject(id: string) {
		selectProject(id);
		selectSession(null);
	}

	async function handleCreateSession(provider: ProviderStatus) {
		if (!activeProjectId) return;

		const createRequestedSession = async () => {
			try {
				const session = await createSession(
					activeProjectId,
					provider.id,
					`${provider.label} session`
				);
				selectSession(session.id);
				showNewSession = false;
			} catch (error) {
				openAlert('Session Error', `Failed to create session:\n${error}`);
			}
		};

		if (sessions.some((session) => session.status === 'running')) {
			openConfirm(
				'Shared Workspace Warning',
				'Another session is already running in this project.\n\nSessions in the same project share the same working tree and branch.\nChanges made by one session may conflict with another.',
				'Start Anyway',
				createRequestedSession
			);
			return;
		}

		await createRequestedSession();
	}

	async function handleRemoveProject(e: Event, id: string) {
		e.stopPropagation();
		openConfirm('Remove Project', 'Remove this project?', 'Remove', async () => {
			await removeProject(id);
		});
	}

	async function handleRemoveSession(e: Event, sessionId: string) {
		e.stopPropagation();
		if (!activeProjectId) return;
		await removeSession(sessionId, activeProjectId);
	}

	function providerLabel(providerId: string): string {
		if (providerId === 'claude_code') return 'Claude';
		if (providerId === 'codex') return 'Codex';
		return providerId;
	}

	function statusIcon(status: string): string {
		if (status === 'running') return '●';
		if (status === 'stopped' || status === 'exited') return '○';
		if (status === 'failed') return '✕';
		return '◌';
	}

	function statusColor(status: string): string {
		if (status === 'running') return '#3fb950';
		if (status === 'failed') return '#f85149';
		return '#8b949e';
	}

	function closeDialog() {
		dialogOpen = false;
		dialogAction = null;
	}

	function openAlert(title: string, message: string) {
		dialogTitle = title;
		dialogMessage = message;
		dialogConfirmLabel = 'Close';
		dialogShowCancel = false;
		dialogAction = () => {
			closeDialog();
		};
		dialogOpen = true;
	}

	function openConfirm(
		title: string,
		message: string,
		confirmLabel: string,
		action: () => void | Promise<void>
	) {
		dialogTitle = title;
		dialogMessage = message;
		dialogConfirmLabel = confirmLabel;
		dialogShowCancel = true;
		dialogAction = async () => {
			closeDialog();
			await action();
		};
		dialogOpen = true;
	}
</script>

<aside class="sidebar">
	<div class="sidebar-header">
		<span class="logo">ADE</span>
		<button class="btn-icon" onclick={handleAddProject} title="Add project">+</button>
	</div>

	<div class="section-label">Projects</div>
	<nav class="project-list">
		{#each projects as project (project.id)}
			<div
				class="project-item"
				class:active={activeProjectId === project.id}
				role="button"
				tabindex="0"
				onclick={() => handleSelectProject(project.id)}
				onkeydown={(e) => e.key === 'Enter' && handleSelectProject(project.id)}
			>
				<span class="project-name">{project.name}</span>
				<span class="project-branch">{project.default_branch ?? ''}</span>
				<span
					class="btn-remove"
					role="button"
					tabindex="0"
					onclick={(e) => handleRemoveProject(e, project.id)}
					onkeydown={(e) => e.key === 'Enter' && handleRemoveProject(e, project.id)}
					title="Remove project"
				>×</span>
			</div>
		{/each}

		{#if projects.length === 0}
			<div class="empty-hint">No projects yet. Click + to add a repo.</div>
		{/if}
	</nav>

	{#if activeProjectId}
		<div class="section-label">
			Sessions
			<button class="btn-icon btn-small" onclick={() => (showNewSession = !showNewSession)}>+</button>
		</div>

		{#if showNewSession}
			<div class="new-session-menu">
				{#each connectedProviders as provider (provider.id)}
					<button class="provider-option" onclick={() => handleCreateSession(provider)}>
						{provider.label}
						{#if provider.version}
							<span class="version">{provider.version}</span>
						{/if}
					</button>
				{/each}
				{#if connectedProviders.length === 0}
					<div class="empty-hint">No providers detected.</div>
				{/if}
			</div>
		{/if}

		<nav class="session-list">
			{#each sessions as session (session.id)}
				<div
					class="session-item"
					class:active={activeSessionId === session.id}
					role="button"
					tabindex="0"
					onclick={() => selectSession(session.id)}
					onkeydown={(e) => e.key === 'Enter' && selectSession(session.id)}
				>
					<span class="session-status" style="color: {statusColor(session.status)}">
						{statusIcon(session.status)}
					</span>
					<span class="session-info">
						<span class="session-title">{session.title}</span>
						<span class="session-provider">{providerLabel(session.provider_id)}</span>
					</span>
					<span
						class="btn-remove"
						role="button"
						tabindex="0"
						onclick={(e) => handleRemoveSession(e, session.id)}
						onkeydown={(e) => e.key === 'Enter' && handleRemoveSession(e, session.id)}
						title="Delete session"
					>×</span>
				</div>
			{/each}
		</nav>
	{/if}

	<div class="sidebar-footer">
		<button class="settings-link" onclick={() => goto('/settings')}>Settings</button>
	</div>
</aside>

<ConfirmDialog
	open={dialogOpen}
	title={dialogTitle}
	message={dialogMessage}
	confirmLabel={dialogConfirmLabel}
	showCancel={dialogShowCancel}
	onCancel={closeDialog}
	onConfirm={() => {
		void dialogAction?.();
	}}
/>

<style>
	.sidebar {
		width: 240px;
		min-width: 200px;
		background: #161b22;
		border-right: 1px solid #30363d;
		display: flex;
		flex-direction: column;
		overflow-y: auto;
	}

	.sidebar-footer {
		margin-top: auto;
		padding: 12px;
		border-top: 1px solid #30363d;
	}

	.settings-link {
		width: 100%;
		padding: 8px 10px;
		border-radius: 8px;
		border: 1px solid #30363d;
		background: #0d1117;
		color: #c9d1d9;
		cursor: pointer;
		text-align: left;
	}

	.settings-link:hover {
		border-color: #58a6ff;
		color: #f0f6fc;
	}

	.sidebar-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 12px 14px;
		border-bottom: 1px solid #30363d;
	}

	.logo {
		font-weight: 700;
		font-size: 1rem;
		color: #f0f6fc;
	}

	.btn-icon {
		background: none;
		border: 1px solid #30363d;
		color: #8b949e;
		width: 26px;
		height: 26px;
		border-radius: 4px;
		cursor: pointer;
		font-size: 1rem;
		display: flex;
		align-items: center;
		justify-content: center;
		line-height: 1;
	}

	.btn-icon:hover {
		color: #f0f6fc;
		border-color: #58a6ff;
	}

	.btn-small {
		width: 20px;
		height: 20px;
		font-size: 0.85rem;
	}

	.section-label {
		padding: 10px 14px 4px;
		font-size: 0.7rem;
		font-weight: 600;
		text-transform: uppercase;
		color: #8b949e;
		letter-spacing: 0.05em;
		display: flex;
		align-items: center;
		justify-content: space-between;
	}

	.project-list,
	.session-list {
		display: flex;
		flex-direction: column;
		padding: 2px 6px;
	}

	.project-item,
	.session-item {
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 6px 8px;
		border: none;
		background: none;
		color: #c9d1d9;
		cursor: pointer;
		border-radius: 4px;
		font-size: 0.82rem;
		text-align: left;
		width: 100%;
	}

	.project-item:hover,
	.session-item:hover {
		background: #21262d;
	}

	.project-item.active,
	.session-item.active {
		background: #1f6feb22;
		color: #58a6ff;
	}

	.project-name {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.project-branch {
		font-size: 0.7rem;
		color: #8b949e;
		flex-shrink: 0;
	}

	.btn-remove {
		background: none;
		border: none;
		color: #8b949e;
		cursor: pointer;
		font-size: 0.9rem;
		padding: 0 2px;
		opacity: 0;
		flex-shrink: 0;
	}

	.project-item:hover .btn-remove,
	.session-item:hover .btn-remove {
		opacity: 1;
	}

	.btn-remove:hover {
		color: #f85149;
	}

	.session-status {
		font-size: 0.6rem;
		flex-shrink: 0;
	}

	.session-info {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
	}

	.session-title {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 0.8rem;
	}

	.session-provider {
		font-size: 0.68rem;
		color: #8b949e;
	}

	.new-session-menu {
		padding: 4px 6px;
	}

	.provider-option {
		display: flex;
		align-items: center;
		gap: 6px;
		width: 100%;
		padding: 6px 8px;
		border: 1px solid #30363d;
		background: #21262d;
		color: #c9d1d9;
		cursor: pointer;
		border-radius: 4px;
		font-size: 0.8rem;
		margin-bottom: 3px;
	}

	.provider-option:hover {
		border-color: #58a6ff;
		color: #f0f6fc;
	}

	.version {
		font-size: 0.68rem;
		color: #8b949e;
		margin-left: auto;
	}

	.empty-hint {
		padding: 8px 14px;
		font-size: 0.75rem;
		color: #484f58;
	}
</style>
