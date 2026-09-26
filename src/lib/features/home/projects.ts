import type { DiscoveredProject, DiscoveredProviderActivity, RecentFolder } from '$lib/types/backend'
import { basename } from '$lib/utils/paths'

/** One Home card: a folder recent to Sworm, to agent CLIs, or both. */
export interface HomeProject {
  path: string
  name: string
  exists: boolean
  /** Last opened in Sworm; null when only agent history knows the folder. */
  openedAt: string | null
  /** Last agent CLI activity; null when only Sworm knows the folder. */
  agentAt: string | null
  providers: DiscoveredProviderActivity[]
  /** Agent sessions per day across providers, oldest first. */
  dailyCounts: number[]
  /** Latest of `openedAt` and `agentAt`; ranks the grid. */
  lastActive: string
}

/** Union Sworm's recents with agent history, most recently active first. */
export function mergeProjects(recent: RecentFolder[], discovered: DiscoveredProject[]): HomeProject[] {
  const byPath = new Map<string, HomeProject>()
  for (const project of discovered) {
    const dailyCounts = [0, 0, 0, 0, 0, 0, 0]
    for (const provider of project.providers) {
      provider.daily_counts.forEach((count, day) => (dailyCounts[day] += count))
    }
    byPath.set(project.path, {
      path: project.path,
      name: project.name,
      exists: project.path_exists,
      openedAt: null,
      agentAt: project.last_active,
      providers: project.providers,
      dailyCounts,
      lastActive: project.last_active
    })
  }
  for (const { path, opened_at } of recent) {
    const known = byPath.get(path)
    if (known) {
      known.openedAt = opened_at
      // Recents are pruned to folders that still resolve.
      known.exists = true
      if (Date.parse(opened_at) > Date.parse(known.lastActive)) known.lastActive = opened_at
    } else {
      byPath.set(path, {
        path,
        name: basename(path),
        exists: true,
        openedAt: opened_at,
        agentAt: null,
        providers: [],
        dailyCounts: [0, 0, 0, 0, 0, 0, 0],
        lastActive: opened_at
      })
    }
  }
  return [...byPath.values()].sort((a, b) => Date.parse(b.lastActive) - Date.parse(a.lastActive))
}
