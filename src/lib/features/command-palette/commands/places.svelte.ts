import type { CommandGroup } from './types'
import { getServers, getWorkbenchSections, workbenchDetail, workbenchTitle } from '$lib/features/browser/places.svelte'
import { browseServer, goToFolder, goToWorkbench, localHostLabel } from '$lib/features/browser/state.svelte'
import { getRecentFolders } from '$lib/features/folders/state.svelte'
import { FolderClockIcon, Layers, ServerIcon } from '$lib/icons/lucideExports'
import { basename, splitRemotePath } from '$lib/utils/paths'

export function getPlaceCommandGroups(): CommandGroup[] {
  return [
    {
      heading: 'Workbenches',
      commands: getWorkbenchSections().flatMap(({ server, workbenches }) =>
        workbenches.map((workbench) => {
          const label = workbenchTitle(workbench)
          const host = server ?? localHostLabel()
          const subtitle = workbenchDetail(workbench, host)
          return {
            id: `place-workbench:${server ?? ''}:${workbench.id}`,
            label,
            subtitle,
            icon: Layers,
            keywords: ['workbench', host, subtitle, workbench.id, ...workbench.folders],
            onSelect: () => void goToWorkbench(workbench, server)
          }
        })
      )
    },
    {
      heading: 'Recent',
      commands: getRecentFolders().map(({ path }) => ({
        id: `place-folder:${path}`,
        label: basename(path),
        subtitle: splitRemotePath(path)?.server ?? path,
        icon: FolderClockIcon,
        keywords: ['recent', 'folder', path],
        onSelect: () => void goToFolder(path)
      }))
    },
    {
      heading: 'Servers',
      commands: [
        {
          id: 'place-server:local',
          label: localHostLabel(),
          icon: ServerIcon,
          keywords: ['local', 'server', 'browse', localHostLabel()],
          onSelect: () => browseServer(null)
        },
        ...getServers().map(({ name, state, lastError }) => ({
          id: `place-server:remote:${name}`,
          label: name,
          subtitle: state,
          icon: ServerIcon,
          serverState: state,
          keywords: ['remote', 'server', 'browse', name, state, lastError ?? ''],
          onSelect: () => browseServer(name)
        }))
      ]
    }
  ].filter((group) => group.commands.length > 0)
}
