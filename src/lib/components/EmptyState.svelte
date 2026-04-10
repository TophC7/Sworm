<script lang="ts">
	import { backend } from '$lib/api/backend';
	import { addProject, selectProject } from '$lib/stores/projects.svelte';

	async function handleOpen() {
		try {
			const path = await backend.projects.selectDirectory();
			if (path) {
				const project = await addProject(path);
				selectProject(project.id);
			}
		} catch (e) {
			alert(`Failed to add project: ${e}`);
		}
	}
</script>

<div class="empty-state">
	<div class="content">
		<h1>ADE</h1>
		<p>Linux-first desktop app for coding-agent CLIs</p>
		<button class="open-btn" onclick={handleOpen}>Open Repository</button>
		<p class="hint">Select a local git repository to get started</p>
	</div>
</div>

<style>
	.empty-state {
		flex: 1;
		display: flex;
		align-items: center;
		justify-content: center;
		background: #0d1117;
	}

	.content {
		text-align: center;
	}

	h1 {
		font-size: 2.5rem;
		color: #f0f6fc;
		margin: 0 0 0.5rem;
	}

	p {
		color: #8b949e;
		margin: 0 0 1.5rem;
	}

	.open-btn {
		padding: 10px 24px;
		background: #238636;
		color: #fff;
		border: none;
		border-radius: 6px;
		font-size: 0.95rem;
		cursor: pointer;
		font-weight: 500;
	}

	.open-btn:hover {
		background: #2ea043;
	}

	.hint {
		font-size: 0.8rem;
		margin-top: 1rem;
	}
</style>
