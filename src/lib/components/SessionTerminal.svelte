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

		const nextManager = sessionRegistry.attach(nextSession.id, containerEl);
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

<div class="session-terminal">
	<div class="terminal-toolbar">
		<span class="terminal-label">
			<span class="status-dot" class:running={isRunning}></span>
			{session.title}
		</span>

		<div class="toolbar-actions">
			{#if isRunning}
				<button class="toolbar-btn stop" onclick={handleStop}>Stop</button>
			{:else}
				<button class="toolbar-btn start" onclick={handleRestart}>Restart</button>
			{/if}
		</div>
	</div>

	{#if error}
		<div class="terminal-error">{error}</div>
	{/if}

	<div class="terminal-container" bind:this={containerEl}></div>
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

<style>
	.session-terminal {
		flex: 1;
		display: flex;
		flex-direction: column;
		background: #0d1117;
		min-height: 0;
	}

	.terminal-toolbar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 4px 10px;
		background: #161b22;
		border-bottom: 1px solid #30363d;
		min-height: 32px;
	}

	.terminal-label {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 0.78rem;
		color: #8b949e;
	}

	.status-dot {
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: #484f58;
	}

	.status-dot.running {
		background: #3fb950;
	}

	.toolbar-actions {
		display: flex;
		gap: 6px;
	}

	.toolbar-btn {
		padding: 2px 10px;
		border: 1px solid #30363d;
		border-radius: 4px;
		font-size: 0.72rem;
		cursor: pointer;
		background: #21262d;
		color: #c9d1d9;
	}

	.toolbar-btn:hover {
		border-color: #58a6ff;
	}

	.toolbar-btn.stop {
		color: #f85149;
	}

	.toolbar-btn.start {
		color: #3fb950;
	}

	.terminal-error {
		padding: 6px 10px;
		background: #2d1014;
		color: #f85149;
		font-size: 0.8rem;
		border-bottom: 1px solid #5b1b21;
	}

	.terminal-container {
		flex: 1;
		min-height: 0;
	}
</style>
