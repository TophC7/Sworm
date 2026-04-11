<script lang="ts">
	import { DiffView, DiffModeEnum } from '@git-diff-view/svelte';
	import '@git-diff-view/svelte/styles/diff-view-pure.css';
	import { parseDiffMeta, extToHighlightLang, isBinaryDiff } from '$lib/utils/diffParser';

	let {
		rawDiff,
		filePath,
		oldContent = null,
		newContent = null,
		onClose
	}: {
		rawDiff: string;
		filePath: string;
		oldContent?: string | null;
		newContent?: string | null;
		onClose: () => void;
	} = $props();

	let mode = $state<DiffModeEnum>(DiffModeEnum.Split);

	let meta = $derived(parseDiffMeta(rawDiff));
	let lang = $derived(extToHighlightLang(filePath));
	let binary = $derived(isBinaryDiff(rawDiff));
	// Each element in hunks is parsed by DiffParser.parse() which expects
	// a COMPLETE diff with headers (---, +++, @@ hunks). Pass the whole
	// raw diff as a single element — don't pre-split.
	let hunks = $derived(rawDiff.trim() ? [rawDiff] : []);

	// The library requires non-empty content strings for #doFile() to run.
	// When content is null (new/deleted file), pass undefined so the library's
	// || '' fallback kicks in and #composeFile() reconstructs from hunks.
	let data = $derived({
		oldFile: {
			fileName: meta.oldFileName ?? filePath,
			fileLang: lang,
			content: oldContent ?? undefined
		},
		newFile: {
			fileName: meta.newFileName ?? filePath,
			fileLang: lang,
			content: newContent ?? undefined
		},
		hunks
	});
</script>

<div class="diff-viewer-root flex flex-col flex-1 min-h-0 overflow-hidden">
	<!-- Toolbar -->
	<div class="flex items-center justify-between gap-2 px-3 py-1.5 bg-surface border-b border-edge text-[0.75rem]">
		<div class="flex items-center gap-2 min-w-0">
			<span class="font-mono text-fg truncate" title={filePath}>{filePath}</span>
		</div>

		<div class="flex items-center gap-2 shrink-0">
			<!-- Split / Unified toggle -->
			<div class="flex items-center bg-ground rounded overflow-hidden border border-edge">
				<button
					class="px-2 py-0.5 text-[0.68rem] transition-colors {mode === DiffModeEnum.Split ? 'bg-accent-bg text-accent' : 'text-muted hover:text-fg'}"
					onclick={() => (mode = DiffModeEnum.Split)}
				>
					Split
				</button>
				<button
					class="px-2 py-0.5 text-[0.68rem] transition-colors {mode === DiffModeEnum.Unified ? 'bg-accent-bg text-accent' : 'text-muted hover:text-fg'}"
					onclick={() => (mode = DiffModeEnum.Unified)}
				>
					Unified
				</button>
			</div>

			<button class="btn-ghost text-[0.8rem]" onclick={onClose} title="Close diff"
				>&times;</button
			>
		</div>
	</div>

	<!-- Diff content -->
	{#if binary}
		<div class="flex-1 flex items-center justify-center text-subtle text-[0.8rem] italic">
			Binary file — diff not available
		</div>
	{:else if !rawDiff.trim()}
		<div class="flex-1 flex items-center justify-center text-subtle text-[0.8rem] italic">
			No changes
		</div>
	{:else}
		<div class="flex-1 overflow-auto min-h-0">
			<DiffView
				{data}
				diffViewMode={mode}
				diffViewTheme="dark"
				diffViewHighlight={true}
				diffViewWrap={false}
				diffViewFontSize={13}
			/>
		</div>
	{/if}
</div>
