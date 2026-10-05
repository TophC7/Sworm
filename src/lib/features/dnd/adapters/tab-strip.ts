import type { Tab } from '$lib/features/workbench/model'
import { dragSource } from '$lib/features/dnd/transfer.svelte'
import { platform } from '$lib/platform'

export function tabDragSource(args: { tab: Tab }) {
  return dragSource(() =>
    args.tab.locked ? null : [{ kind: 'tab', tabId: args.tab.id, sourceWindowLabel: platform.workbench.id }]
  )
}

/** The server tab drags its whole group. */
export function groupDragSource(args: { server: string; workbenchId: string }) {
  return dragSource(() => [{
    kind: 'workbench',
    server: args.server,
    workbenchId: args.workbenchId,
    sourceWindowLabel: platform.workbench.id
  }])
}
