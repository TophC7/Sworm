// Project state module using Svelte 5 runes.
//
// Shared across the app for project list and active project selection.

import { backend } from '$lib/api/backend';
import type { Project } from '$lib/types/backend';

let projects = $state<Project[]>([]);
let activeProjectId = $state<string | null>(null);

// Derived: active project object
let activeProject = $derived(projects.find((p) => p.id === activeProjectId) ?? null);

export function getProjects() {
	return projects;
}

export function getActiveProject() {
	return activeProject;
}

export function getActiveProjectId() {
	return activeProjectId;
}

export async function loadProjects() {
	try {
		projects = await backend.projects.list();
		// If active project is gone, deselect
		if (activeProjectId && !projects.find((p) => p.id === activeProjectId)) {
			activeProjectId = null;
		}
	} catch (e) {
		console.error('Failed to load projects:', e);
	}
}

export function selectProject(id: string | null) {
	activeProjectId = id;
}

export async function addProject(path: string): Promise<Project> {
	const project = await backend.projects.add(path);
	await loadProjects();
	return project;
}

export async function removeProject(id: string) {
	await backend.projects.remove(id);
	if (activeProjectId === id) {
		activeProjectId = null;
	}
	await loadProjects();
}
