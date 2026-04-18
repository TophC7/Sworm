import { DND_MIME, type DragPayload, type SwormDragKind } from '$lib/dnd/payload'
import { createHoverStore } from '$lib/dnd/hover-state.svelte'
import { computeZone, type Zone } from '$lib/dnd/overlay'
import { dragObserver, frameAt, type DragFrame } from '$lib/dnd/observer.svelte'
import { DropRegistry } from '$lib/dnd/registry.svelte'
import { LocalTransfer } from '$lib/dnd/transfer.svelte'
import { backend } from '$lib/api/backend'
import {
  addEditorTab,
  canSplitPane,
  moveTabToPane,
  setActiveTab,
  setFocusedPane,
  splitPaneAt,
  type SplitDirection,
  type PaneSlot,
  type PaneState
} from '$lib/stores/workspace.svelte'
import { notify } from '$lib/stores/notifications.svelte'
import { toProjectRelativePath } from '$lib/utils/paths'

interface PaneDropObserverArgs {
  pane: PaneState
  projectId: string
  projectPath: string
  locked: boolean
  onDropHandled?: () => void
}

interface PaneZoneState {
  visible: boolean
  zone: Zone
  label: string
}

const paneZones = createHoverStore<PaneZoneState>()

function zonesEqual(a: PaneZoneState, b: PaneZoneState): boolean {
  return a.visible === b.visible && a.zone === b.zone && a.label === b.label
}

function paneKey(projectId: string, paneSlot: PaneSlot): string {
  return `${projectId}:${paneSlot}`
}

function paneTargetId(projectId: string, paneSlot: PaneSlot): string {
  return `pane:${projectId}:${paneSlot}`
}

function setPaneZone(projectId: string, paneSlot: PaneSlot, zone: Zone, label: string): void {
  // Deduped so `dragover` (once per rAF) doesn't churn reactive reads.
  paneZones.set(paneKey(projectId, paneSlot), { visible: true, zone, label }, zonesEqual)
}

function clearPaneZone(projectId: string, paneSlot: PaneSlot): void {
  paneZones.clear(paneKey(projectId, paneSlot))
}

function canAcceptPayload(payload: DragPayload | null, projectId: string, locked: boolean): boolean {
  if (locked) return false
  if (!payload) return false
  return payload.items.some((item) => {
    if (item.kind === 'tab') return item.projectId === projectId
    if (item.kind === 'file') return item.projectId === projectId && !item.isDir
    if (item.kind === 'os-files') return item.paths.length > 0
    return false
  })
}

function resolveDropZone(
  frame: DragFrame,
  projectId: string,
  paneSlot: PaneSlot
): { zone: Zone; label: string; splitDirection: SplitDirection | null } {
  const canSplitLeft = canSplitPane(projectId, paneSlot, 'left')
  const canSplitRight = canSplitPane(projectId, paneSlot, 'right')
  const canSplitUp = canSplitPane(projectId, paneSlot, 'up')
  const canSplitDown = canSplitPane(projectId, paneSlot, 'down')
  const rawZone = computeZone(frame.localX, frame.localY, frame.width, frame.height, {
    allowSplit: canSplitLeft || canSplitRight || canSplitUp || canSplitDown,
    edgeMarginRatio: 0.15
  })

  if (rawZone === 'left' && canSplitLeft) {
    return { zone: rawZone, label: 'Split Left', splitDirection: 'left' }
  }
  if (rawZone === 'right' && canSplitRight) {
    return { zone: rawZone, label: 'Split Right', splitDirection: 'right' }
  }
  if (rawZone === 'up' && canSplitUp) {
    return { zone: rawZone, label: 'Split Up', splitDirection: 'up' }
  }
  if (rawZone === 'down' && canSplitDown) {
    return { zone: rawZone, label: 'Split Down', splitDirection: 'down' }
  }

  return { zone: 'merge', label: 'Move Here', splitDirection: null }
}

async function dispatchPaneDrop(
  payload: DragPayload,
  projectId: string,
  projectPath: string,
  paneSlot: PaneSlot,
  locked: boolean,
  splitDirection: SplitDirection | null
): Promise<boolean> {
  if (locked) return false
  if (!canAcceptPayload(payload, projectId, locked)) return false

  let targetSlot = paneSlot
  if (splitDirection) {
    const splitSlot = splitPaneAt(projectId, paneSlot, splitDirection)
    if (splitSlot) {
      targetSlot = splitSlot
    }
  }

  let skippedExternalCount = 0
  let skippedExternalDirs = 0
  let handled = false
  for (const item of payload.items) {
    if (item.kind === 'tab') {
      if (item.projectId !== projectId) continue
      moveTabToPane(projectId, item.tabId, targetSlot)
      setActiveTab(projectId, targetSlot, item.tabId)
      setFocusedPane(targetSlot)
      handled = true
      continue
    }

    if (item.kind === 'file') {
      if (item.projectId !== projectId || item.isDir) continue
      const tabId = addEditorTab(projectId, item.path)
      moveTabToPane(projectId, tabId, targetSlot)
      setActiveTab(projectId, targetSlot, tabId)
      setFocusedPane(targetSlot)
      handled = true
      continue
    }

    if (item.kind === 'os-files') {
      // Stat in parallel — drops of many files otherwise serialize into
      // one round-trip per path.
      const relatives = item.paths.map((abs) => toProjectRelativePath(projectPath, abs))
      const stats = await Promise.all(
        relatives.map((rel) => (rel ? backend.files.stat(projectPath, rel) : Promise.resolve(null)))
      )

      for (let i = 0; i < relatives.length; i++) {
        const rel = relatives[i]
        if (!rel) {
          skippedExternalCount += 1
          continue
        }
        const stat = stats[i]
        if (!stat || stat.isDir) {
          skippedExternalDirs += 1
          continue
        }

        const tabId = addEditorTab(projectId, rel)
        moveTabToPane(projectId, tabId, targetSlot)
        setActiveTab(projectId, targetSlot, tabId)
        setFocusedPane(targetSlot)
        handled = true
      }
    }
  }

  if (skippedExternalCount > 0) {
    notify.info(
      'Skipped external files',
      `${skippedExternalCount} file${skippedExternalCount === 1 ? '' : 's'} are outside this project and cannot be opened directly.`
    )
  }
  if (skippedExternalDirs > 0) {
    notify.info(
      'Skipped dropped folders',
      `${skippedExternalDirs} folder${skippedExternalDirs === 1 ? '' : 's'} cannot be opened as editor tabs.`
    )
  }

  return handled
}

export const paneDndUi = {
  visible(projectId: string, paneSlot: PaneSlot): boolean {
    return paneZones.get(paneKey(projectId, paneSlot))?.visible ?? false
  },
  zone(projectId: string, paneSlot: PaneSlot): Zone {
    return paneZones.get(paneKey(projectId, paneSlot))?.zone ?? 'merge'
  },
  label(projectId: string, paneSlot: PaneSlot): string {
    return paneZones.get(paneKey(projectId, paneSlot))?.label ?? 'Move Here'
  },
  clear(projectId: string, paneSlot: PaneSlot): void {
    clearPaneZone(projectId, paneSlot)
  }
}

export function paneDropObserver(args: PaneDropObserverArgs) {
  const targetId = paneTargetId(args.projectId, args.pane.slot)

  const observer = dragObserver({
    accept: (payload, types) => {
      if (payload) return canAcceptPayload(payload, args.projectId, args.locked)
      if (args.locked) return false
      return types.includes(DND_MIME.FILES)
    },
    onOver: (_event, frame) => {
      const payload = LocalTransfer.peek()
      const owner = payload ? DropRegistry.findAt(frame.clientX, frame.clientY, payload) : null
      if (owner && owner.id !== targetId) {
        clearPaneZone(args.projectId, args.pane.slot)
        return
      }

      const { zone, label } = resolveDropZone(frame, args.projectId, args.pane.slot)
      setPaneZone(args.projectId, args.pane.slot, zone, label)
    },
    onLeave: () => {
      clearPaneZone(args.projectId, args.pane.slot)
    },
    onDrop: async (_event, payload, frame) => {
      if (!frame) return
      const owner = DropRegistry.findAt(frame.clientX, frame.clientY, payload)
      if (owner && owner.id !== targetId) {
        clearPaneZone(args.projectId, args.pane.slot)
        return
      }

      const { zone, splitDirection } = resolveDropZone(frame, args.projectId, args.pane.slot)
      clearPaneZone(args.projectId, args.pane.slot)
      const handled = await dispatchPaneDrop(
        payload,
        args.projectId,
        args.projectPath,
        args.pane.slot,
        args.locked,
        splitDirection
      )
      if (handled) args.onDropHandled?.()
    }
  })

  return (element: HTMLElement) => {
    const disposeObserver = observer(element)
    const disposeRegistry = DropRegistry.register({
      id: targetId,
      element,
      accept: (payload) => canAcceptPayload(payload, args.projectId, args.locked),
      hover: (payload, clientX, clientY) => {
        if (args.locked) return
        if (!canAcceptPayload(payload, args.projectId, args.locked)) return
        const frame = frameAt(element, clientX, clientY)
        if (!frame) return
        const { zone, label } = resolveDropZone(frame, args.projectId, args.pane.slot)
        setPaneZone(args.projectId, args.pane.slot, zone, label)
      },
      leave: () => {
        clearPaneZone(args.projectId, args.pane.slot)
      },
      dispatch: async (payload, clientX, clientY) => {
        if (args.locked) return
        const frame = frameAt(element, clientX, clientY)
        const splitDirection = frame ? resolveDropZone(frame, args.projectId, args.pane.slot).splitDirection : null
        clearPaneZone(args.projectId, args.pane.slot)
        const handled = await dispatchPaneDrop(
          payload,
          args.projectId,
          args.projectPath,
          args.pane.slot,
          args.locked,
          splitDirection
        )
        if (handled) args.onDropHandled?.()
      }
    })

    return () => {
      disposeRegistry()
      clearPaneZone(args.projectId, args.pane.slot)
      disposeObserver()
    }
  }
}

export function dragKindsForPane(payload: DragPayload): SwormDragKind['kind'][] {
  return payload.items.map((item) => item.kind)
}
