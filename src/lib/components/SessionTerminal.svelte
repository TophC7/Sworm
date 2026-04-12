<script lang="ts">
	import '@xterm/xterm/css/xterm.css';
	import { onMount } from 'svelte';
	import type { Session, SessionStatus } from '$lib/types/backend';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import { getSessions } from '$lib/stores/sessions.svelte';
	import * as sessionRegistry from '$lib/terminal/sessionRegistry';
	import type { TerminalSessionManager } from '$lib/terminal/TerminalSessionManager';

	let {
		session,
		onStatusChange
	}: {
		session: Session;
		onStatusChange?: (status: SessionStatus) => void;
	} = $props();

	let containerEl: HTMLDivElement | undefined = $state(undefined);
	let manager: TerminalSessionManager | null = $state(null);
	let isRunning = $state(false);
	let error = $state<string | null>(null);
	let sessions = $derived(getSessions());
	let warningOpen = $state(false);
	let pendingStartAction: (() => Promise<void>) | null = null;

	let mounted = false;
	let attachedSessionId: string | null = null;
	let cleanupEventListener: (() => void) | null = null;
	let cleanupErrorListener: (() => void) | null = null;

	function clearManagerListeners() {
		cleanupEventListener?.();
		cleanupEventListener = null;
		cleanupErrorListener?.();
		cleanupErrorListener = null;
	}

	function bindManager(nextManager: TerminalSessionManager) {
		clearManagerListeners();

		manager = nextManager;
		isRunning = nextManager.isPtyActive();
		error = nextManager.getLastError();

		cleanupEventListener = nextManager.registerEventListener((event) => {
			if (event.type === 'started') {
				isRunning = true;
				error = null;
				onStatusChange?.('running');
			}

			if (event.type === 'exit') {
				isRunning = false;
				onStatusChange?.('exited');
			}
		});

		cleanupErrorListener = nextManager.registerErrorListener((message) => {
			error = message;
		});
	}

	async function attachSession(nextSession: Session) {
		if (!mounted || !containerEl) {
			return;
		}

		const switchingSessions = attachedSessionId !== nextSession.id;

		if (attachedSessionId && attachedSessionId !== nextSession.id) {
			sessionRegistry.detach(attachedSessionId);
			clearManagerListeners();
		}

		const nextManager = await sessionRegistry.attach(nextSession.id, containerEl);
		attachedSessionId = nextSession.id;
		bindManager(nextManager);

		const shouldAutoStart =
			switchingSessions &&
			!nextManager.isPtyActive() &&
			(nextSession.status === 'idle' || nextSession.status === 'running');

		if (shouldAutoStart) {
			try {
				await nextManager.startPty(nextSession);
			} catch (startError) {
				error = String(startError);
				isRunning = false;
				onStatusChange?.('failed');
			}
		}
	}

	onMount(() => {
		mounted = true;
		void attachSession(session);

		return () => {
			if (attachedSessionId) {
				sessionRegistry.detach(attachedSessionId);
			}
			clearManagerListeners();
		};
	});

	$effect(() => {
		if (!mounted || !containerEl) {
			return;
		}

		void attachSession(session);
	});

	async function handleStop() {
		if (!manager) {
			return;
		}

		try {
			await manager.stopPty();
			isRunning = false;
			onStatusChange?.('stopped');
		} catch (stopError) {
			error = String(stopError);
		}
	}

	async function handleRestart() {
		if (!manager) {
			return;
		}

		const activeManager = manager;
		const restart = async () => {
			try {
				if (activeManager.isPtyActive()) {
					await activeManager.stopPty();
					onStatusChange?.('stopped');
				}

				isRunning = false;
				error = null;
				await activeManager.startPty(session);
			} catch (restartError) {
				error = String(restartError);
				isRunning = false;
				onStatusChange?.('failed');
			}
		};

		if (sessions.some((candidate) => candidate.id !== session.id && candidate.status === 'running')) {
			pendingStartAction = restart;
			warningOpen = true;
			return;
		}

		await restart();
	}
</script>

<div class="flex-1 flex flex-col bg-ground min-h-0">
	<div class="flex items-center justify-between px-2.5 py-1 bg-surface border-b border-edge min-h-8">
		<span class="flex items-center gap-1.5 text-[0.78rem] text-muted">
			<span
				class="w-[7px] h-[7px] rounded-full {isRunning ? 'bg-success' : 'bg-subtle'}"
			></span>
			{session.title}
		</span>

		<div class="flex gap-1.5">
			{#if isRunning}
				<button
					class="py-0.5 px-2.5 border border-edge rounded text-[0.72rem] cursor-pointer bg-raised text-danger hover:border-accent transition-colors"
					onclick={handleStop}
				>
					Stop
				</button>
			{:else}
				<button
					class="py-0.5 px-2.5 border border-edge rounded text-[0.72rem] cursor-pointer bg-raised text-success hover:border-accent transition-colors"
					onclick={handleRestart}
				>
					Restart
				</button>
			{/if}
		</div>
	</div>

	{#if error}
		<div class="px-2.5 py-1.5 bg-danger-bg text-danger text-[0.8rem] border-b border-danger-border">
			{error}
		</div>
	{/if}

	<div class="flex-1 min-h-0" bind:this={containerEl}></div>
</div>

<ConfirmDialog
	open={warningOpen}
	title="Shared Workspace Warning"
	message="Another session is already running in this project.\n\nSessions in the same project share the same working tree and branch.\nChanges made by one session may conflict with another."
	confirmLabel="Start Anyway"
	onCancel={() => {
		warningOpen = false;
		pendingStartAction = null;
	}}
	onConfirm={() => {
		const action = pendingStartAction;
		warningOpen = false;
		pendingStartAction = null;
		if (action) {
			void action();
		}
	}}
/>
