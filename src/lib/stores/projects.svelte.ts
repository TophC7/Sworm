// Project state module using Svelte 5 runes.
//
// Manages the project list (CRUD against the backend).
// Active project selection is delegated to workspace.svelte.ts —
// this module re-exports getters for backward compatibility.

import { backend } from '$lib/api/backend'
import type { Project } from '$lib/types/backend'
import {
  getActiveProjectId as workspaceGetActiveProjectId,
  selectProject as workspaceSelectProject,
  openProject
} from '$lib/stores/workspace.svelte'

let projects = $state<Project[]>([])

// Derived: active project reads from workspace's activeProjectId
let activeProject = $derived(projects.find((p) => p.id === workspaceGetActiveProjectId()) ?? null)

export function getProjects() {
  return projects
}

export function getActiveProject() {
  return activeProject
}

export function getActiveProjectId() {
  return workspaceGetActiveProjectId()
}

export function getProjectById(id: string): Project | undefined {
  return projects.find((p) => p.id === id)
}

export function selectProject(id: string | null) {
  workspaceSelectProject(id)
}

// --- Backend CRUD ---

export async function loadProjects() {
  try {
    projects = await backend.projects.list()
  } catch (e) {
    console.error('Failed to load projects:', e)
  }
}

export async function addProject(path: string): Promise<Project> {
  // Ensure we have the latest list before checking for duplicates
  await loadProjects()

  const normalizedPath = path.replace(/\/+$/, '')
  const existing = projects.find((p) => p.path.replace(/\/+$/, '') === normalizedPath)
  if (existing) return existing

  try {
    const project = await backend.projects.add(path)
    await loadProjects()
    return project
  } catch (e) {
    // Backend rejected — likely a duplicate with a different path form.
    // Re-fetch and try exact match before giving up.
    await loadProjects()
    const found = projects.find((p) => p.path.replace(/\/+$/, '') === normalizedPath)
    if (found) return found
    throw e
  }
}

export async function removeProject(id: string) {
  await backend.projects.remove(id)
  await loadProjects()
}
