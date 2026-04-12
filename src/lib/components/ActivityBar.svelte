<script lang="ts">
	import {
		getSidebarView,
		setSidebarView,
		isGitSidebarCollapsed,
		setGitSidebarCollapsed,
		type SidebarView
	} from '$lib/stores/ui.svelte'
	import GitBranchIcon from '@lucide/svelte/icons/git-branch'
	import History from '@lucide/svelte/icons/history'
	import { cn } from '$lib/utils/cn'

	let activeView = $derived(getSidebarView())
	let collapsed = $derived(isGitSidebarCollapsed())

	function handleClick(view: SidebarView) {
		if (activeView === view) {
			// Toggle collapse when clicking the already-active view
			setGitSidebarCollapsed(!collapsed)
		} else {
			// Switch view; expand if collapsed
			setSidebarView(view)
			if (collapsed) setGitSidebarCollapsed(false)
		}
	}

	const views: { id: SidebarView; icon: typeof GitBranchIcon; label: string }[] = [
		{ id: 'git', icon: GitBranchIcon, label: 'Git' },
		{ id: 'sessions', icon: History, label: 'Sessions' }
	]
</script>

<div class="flex flex-col items-center w-9 shrink-0 bg-ground border-r border-edge py-2 gap-0.5">
	{#each views as view}
		{@const isActive = activeView === view.id && !collapsed}
		<button
			class={cn(
				'relative flex items-center justify-center w-7 h-7 rounded-sm transition-colors cursor-pointer',
				isActive
					? 'text-bright bg-surface'
					: 'text-muted hover:text-subtle hover:bg-surface/50'
			)}
			onclick={() => handleClick(view.id)}
			title={view.label}
		>
			{#if isActive}
				<span class="absolute left-0 top-1 bottom-1 w-0.5 rounded-r bg-accent"></span>
			{/if}
			<view.icon size={16} />
		</button>
	{/each}
</div>
