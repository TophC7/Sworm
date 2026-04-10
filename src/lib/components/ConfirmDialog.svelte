<script lang="ts">
	let {
		open = false,
		title,
		message,
		confirmLabel = 'Confirm',
		cancelLabel = 'Cancel',
		showCancel = true,
		onConfirm,
		onCancel
	}: {
		open?: boolean;
		title: string;
		message: string;
		confirmLabel?: string;
		cancelLabel?: string;
		showCancel?: boolean;
		onConfirm?: () => void;
		onCancel?: () => void;
	} = $props();
</script>

{#if open}
	<div
		class="dialog-backdrop"
		role="presentation"
		tabindex="-1"
		onclick={onCancel}
		onkeydown={(event) => {
			if (event.key === 'Escape') {
				onCancel?.();
			}
		}}
	>
		<div
			class="dialog-card"
			role="dialog"
			aria-modal="true"
			aria-labelledby="dialog-title"
			tabindex="0"
			onclick={(event) => event.stopPropagation()}
			onkeydown={(event) => event.stopPropagation()}
		>
			<h2 id="dialog-title">{title}</h2>
			<p>{message}</p>

			<div class="actions">
				{#if showCancel}
					<button class="secondary" onclick={onCancel}>{cancelLabel}</button>
				{/if}
				<button class="primary" onclick={onConfirm}>{confirmLabel}</button>
			</div>
		</div>
	</div>
{/if}

<style>
	.dialog-backdrop {
		position: fixed;
		inset: 0;
		background: rgba(1, 4, 9, 0.72);
		backdrop-filter: blur(6px);
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 20px;
		z-index: 50;
	}

	.dialog-card {
		width: min(480px, 100%);
		border-radius: 18px;
		padding: 20px;
		background: #161b22;
		border: 1px solid #30363d;
		box-shadow: 0 30px 80px rgba(0, 0, 0, 0.45);
	}

	h2 {
		margin: 0 0 10px;
		font-size: 1rem;
		color: #f0f6fc;
	}

	p {
		margin: 0;
		color: #8b949e;
		line-height: 1.5;
		white-space: pre-line;
	}

	.actions {
		display: flex;
		justify-content: flex-end;
		gap: 10px;
		margin-top: 20px;
	}

	button {
		border-radius: 10px;
		padding: 9px 14px;
		border: 1px solid #30363d;
		cursor: pointer;
	}

	.secondary {
		background: transparent;
		color: #c9d1d9;
	}

	.primary {
		background: #1f6feb;
		color: #f0f6fc;
		border-color: #1f6feb;
	}
</style>
