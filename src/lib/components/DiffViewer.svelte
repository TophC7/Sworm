<script lang="ts">
	import { DiffView, DiffModeEnum } from '@git-diff-view/svelte';
	import '@git-diff-view/svelte/styles/diff-view-pure.css';
	import { parseDiffMeta, extToHighlightLang, isBinaryDiff } from '$lib/utils/diffParser';
	import { Button } from '$lib/components/ui/button';
	import { TabsRoot, TabsList, TabsTrigger } from '$lib/components/ui/tabs';

	let {
		rawDiff,
		filePath,
		oldContent = null,
		newContent = null
	}: {
		rawDiff: string;
		filePath: string;
		oldContent?: string | null;
		newContent?: string | null;
	} = $props();

	let mode = $state<DiffModeEnum>(DiffModeEnum.Split);
	let wrap = $state(false);
	let fontSize = $state(13);

	let meta = $derived(parseDiffMeta(rawDiff));
	let lang = $derived(extToHighlightLang(filePath));
	let binary = $derived(isBinaryDiff(rawDiff));
	let hasDiffContent = $derived(rawDiff.trim().length > 0);
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

	function adjustFontSize(delta: number) {
		fontSize = Math.max(10, Math.min(20, fontSize + delta));
	}
</script>

<div class="diff-viewer-root flex flex-col flex-1 min-h-0 overflow-hidden">
	<!-- Toolbar -->
	<div class="flex items-center justify-between gap-2 border-b border-edge bg-surface px-3 py-1.5 text-[0.75rem]">
		<div class="flex min-w-0 items-center gap-2">
			<span class="font-mono text-fg truncate" title={filePath}>{filePath}</span>
		</div>

		<div class="flex shrink-0 items-center gap-2">
			<!-- Split / Unified toggle -->
			<TabsRoot
				value={mode === DiffModeEnum.Split ? 'split' : 'unified'}
				onValueChange={(v) => {
					mode = v === 'split' ? DiffModeEnum.Split : DiffModeEnum.Unified;
				}}
			>
				<TabsList>
					<TabsTrigger value="split">Split</TabsTrigger>
					<TabsTrigger value="unified">Unified</TabsTrigger>
				</TabsList>
			</TabsRoot>

			<Button
				variant={wrap ? 'accent' : 'ghost'}
				size="xs"
				onclick={() => (wrap = !wrap)}
				aria-pressed={wrap}
				title="Toggle line wrapping"
			>
				Wrap
			</Button>

			<div class="flex items-center gap-1">
				<Button
					variant="ghost"
					size="xs"
					onclick={() => adjustFontSize(-1)}
					disabled={fontSize <= 10}
					title="Decrease diff font size"
				>
					A-
				</Button>
				<span class="min-w-[2.8rem] text-center text-muted tabular-nums">{fontSize}px</span>
				<Button
					variant="ghost"
					size="xs"
					onclick={() => adjustFontSize(1)}
					disabled={fontSize >= 20}
					title="Increase diff font size"
				>
					A+
				</Button>
			</div>

		</div>
	</div>

	<!-- Diff content -->
	{#if binary}
		<div class="flex-1 flex items-center justify-center text-subtle text-[0.8rem] italic">
			Binary file — diff not available
		</div>
	{:else if !hasDiffContent}
		<div class="flex-1 flex items-center justify-center text-subtle text-[0.8rem] italic">
			No changes
		</div>
	{:else}
		<div class="flex-1 min-h-0 overflow-hidden">
			<DiffView
				class="h-full min-h-0"
				{data}
				diffViewMode={mode}
				diffViewTheme="dark"
				diffViewHighlight={true}
				diffViewWrap={wrap}
				diffViewFontSize={fontSize}
			/>
		</div>
	{/if}
</div>
