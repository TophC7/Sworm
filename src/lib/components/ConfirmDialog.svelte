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
		class="fixed inset-0 bg-ground/70 backdrop-blur-sm flex items-center justify-center p-5 z-50"
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
			class="w-full max-w-[480px] rounded-2xl p-5 bg-surface border border-edge shadow-[0_30px_80px_rgba(0,0,0,0.45)]"
			role="dialog"
			aria-modal="true"
			aria-labelledby="dialog-title"
			tabindex="0"
			onclick={(event) => event.stopPropagation()}
			onkeydown={(event) => event.stopPropagation()}
		>
			<h2 id="dialog-title" class="m-0 mb-2.5 text-base text-bright">{title}</h2>
			<p class="m-0 text-muted leading-relaxed whitespace-pre-line">{message}</p>

			<div class="flex justify-end gap-2.5 mt-5">
				{#if showCancel}
					<button
						class="rounded-[10px] py-2 px-3.5 border border-edge cursor-pointer bg-transparent text-fg hover:border-accent transition-colors"
						onclick={onCancel}
					>
						{cancelLabel}
					</button>
				{/if}
				<button
					class="rounded-[10px] py-2 px-3.5 border border-accent-dim cursor-pointer bg-accent-dim text-ground hover:bg-accent transition-colors"
					onclick={onConfirm}
				>
					{confirmLabel}
				</button>
			</div>
		</div>
	</div>
{/if}
