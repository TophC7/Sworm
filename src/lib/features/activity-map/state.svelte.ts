// Activity Map state module.
//
// Discovers project folders where external agent CLIs have been used
// by scanning their conversation history on disk.

import { backend } from '$lib/api/backend'
import type { DiscoveredProject } from '$lib/types/backend'

let projects = $state<DiscoveredProject[]>([])
let loading = $state(false)
let loaded = $state(false)

export function getDiscoveredProjects() {
  return projects
}

export function isActivityMapLoading() {
  return loading
}

export async function loadActivityMap() {
  if (loaded || loading) return
  loading = true
  try {
    projects = await backend.activityMap.get()
    loaded = true
  } catch (e) {
    console.error('Failed to load activity map:', e)
  } finally {
    loading = false
  }
}

export async function refreshActivityMap() {
  loading = true
  try {
    projects = await backend.activityMap.refresh()
    loaded = true
  } catch (e) {
    console.error('Failed to refresh activity map:', e)
    throw e
  } finally {
    loading = false
  }
}

/** Invalidate cache so next loadActivityMap() rescans. */
export function invalidateActivityMap() {
  loaded = false
}
