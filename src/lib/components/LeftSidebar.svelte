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

	function statusColorClass(status: string): string {
		if (status === 'running') return 'text-success';
		if (status === 'failed') return 'text-danger';
		return 'text-muted';
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

<aside class="w-60 min-w-[200px] bg-surface border-r border-edge flex flex-col overflow-y-auto">
	<div class="flex items-center justify-between px-3.5 py-3 border-b border-edge">
		<span class="font-bold text-base text-bright">ADE</span>
		<button
			class="bg-transparent border border-edge text-muted w-[26px] h-[26px] rounded flex items-center justify-center cursor-pointer text-base leading-none hover:text-bright hover:border-accent transition-colors"
			onclick={handleAddProject}
			title="Add project"
		>+</button>
	</div>

	<div class="section-label">
		Projects
	</div>

	<nav class="flex flex-col px-1.5 py-0.5">
		{#each projects as project (project.id)}
			<div
				class="group flex items-center gap-1.5 px-2 py-1.5 cursor-pointer rounded text-[0.82rem] text-left w-full hover:bg-raised {activeProjectId === project.id ? 'bg-accent-bg text-accent' : 'bg-transparent text-fg'}"
				role="button"
				tabindex="0"
				onclick={() => handleSelectProject(project.id)}
				onkeydown={(e) => e.key === 'Enter' && handleSelectProject(project.id)}
			>
				<span class="flex-1 min-w-0 truncate">{project.name}</span>
				<span class="text-[0.7rem] text-muted shrink-0">{project.default_branch ?? ''}</span>
				<span
					class="border-none cursor-pointer text-[0.9rem] px-0.5 opacity-0 shrink-0 text-muted group-hover:opacity-100 hover:text-danger transition-all"
					role="button"
					tabindex="0"
					onclick={(e) => handleRemoveProject(e, project.id)}
					onkeydown={(e) => e.key === 'Enter' && handleRemoveProject(e, project.id)}
					title="Remove project"
				>&times;</span>
			</div>
		{/each}

		{#if projects.length === 0}
			<div class="px-3.5 py-2 text-[0.75rem] text-subtle">No projects yet. Click + to add a repo.</div>
		{/if}
	</nav>

	{#if activeProjectId}
		<div class="section-label">
			Sessions
			<button
				class="bg-transparent border border-edge text-muted w-5 h-5 rounded flex items-center justify-center cursor-pointer text-[0.85rem] leading-none hover:text-bright hover:border-accent transition-colors"
				onclick={() => (showNewSession = !showNewSession)}
			>+</button>
		</div>

		{#if showNewSession}
			<div class="px-1.5 py-1">
				{#each connectedProviders as provider (provider.id)}
					<button
						class="flex items-center gap-1.5 w-full px-2 py-1.5 border border-edge bg-raised text-fg cursor-pointer rounded text-[0.8rem] mb-0.5 hover:border-accent hover:text-bright transition-colors"
						onclick={() => handleCreateSession(provider)}
					>
						{provider.label}
						{#if provider.version}
							<span class="text-[0.68rem] text-muted ml-auto">{provider.version}</span>
						{/if}
					</button>
				{/each}
				{#if connectedProviders.length === 0}
					<div class="px-3.5 py-2 text-[0.75rem] text-subtle">No providers detected.</div>
				{/if}
			</div>
		{/if}

		<nav class="flex flex-col px-1.5 py-0.5">
			{#each sessions as session (session.id)}
				<div
					class="group flex items-center gap-1.5 px-2 py-1.5 cursor-pointer rounded text-[0.82rem] text-left w-full hover:bg-raised {activeSessionId === session.id ? 'bg-accent-bg text-accent' : 'bg-transparent text-fg'}"
					role="button"
					tabindex="0"
					onclick={() => selectSession(session.id)}
					onkeydown={(e) => e.key === 'Enter' && selectSession(session.id)}
				>
					<span class="text-[0.6rem] shrink-0 {statusColorClass(session.status)}">
						{statusIcon(session.status)}
					</span>
					<span class="flex-1 min-w-0 flex flex-col">
						<span class="truncate text-[0.8rem]">{session.title}</span>
						<span class="text-[0.68rem] text-muted">{providerLabel(session.provider_id)}</span>
					</span>
					<span
						class="border-none cursor-pointer text-[0.9rem] px-0.5 opacity-0 shrink-0 text-muted group-hover:opacity-100 hover:text-danger transition-all"
						role="button"
						tabindex="0"
						onclick={(e) => handleRemoveSession(e, session.id)}
						onkeydown={(e) => e.key === 'Enter' && handleRemoveSession(e, session.id)}
						title="Delete session"
					>&times;</span>
				</div>
			{/each}
		</nav>
	{/if}

	<div class="mt-auto p-3 border-t border-edge">
		<button
			class="w-full px-2.5 py-2 rounded-lg border border-edge bg-ground text-fg cursor-pointer text-left hover:border-accent hover:text-bright transition-colors"
			onclick={() => goto('/settings')}
		>Settings</button>
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
