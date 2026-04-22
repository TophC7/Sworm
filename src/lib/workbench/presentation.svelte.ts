import type { Tab } from '$lib/workbench/state.svelte'
import { canLockTab } from '$lib/workbench/state.svelte'
import { getDiffTabTitle } from '$lib/surfaces/diff/service.svelte'
import { getLauncherTitle } from '$lib/surfaces/launcher/service.svelte'
import { getSessionProviderIcon, getSessionTabTitle } from '$lib/surfaces/session/service.svelte'
import { getTextTabFileName, getTextTabTitle } from '$lib/surfaces/text/service.svelte'
import { getToolTabTitle } from '$lib/surfaces/tool/service.svelte'
import { getSurfaceKind, isSurfacePreview, type SurfaceKind } from '$lib/workbench/surfaces'

export interface TabPresentation {
  surfaceKind: SurfaceKind
  title: string
  preview: boolean
  providerIcon: string | null
  fileName: string | null
  lockable: boolean
}

export function getTabPresentation(tab: Tab): TabPresentation {
  const surfaceKind = getSurfaceKind(tab)

  switch (surfaceKind) {
    case 'launcher':
      return {
        surfaceKind,
        title: getLauncherTitle(),
        preview: false,
        providerIcon: null,
        fileName: null,
        lockable: false
      }
    case 'session':
      if (tab.kind !== 'session') throw new Error('Invalid session tab presentation request')
      return {
        surfaceKind,
        title: getSessionTabTitle(tab),
        preview: false,
        providerIcon: getSessionProviderIcon(tab),
        fileName: null,
        lockable: canLockTab(tab)
      }
    case 'text':
      if (tab.kind !== 'text') throw new Error('Invalid text tab presentation request')
      return {
        surfaceKind,
        title: getTextTabTitle(tab),
        preview: isSurfacePreview(tab),
        providerIcon: null,
        fileName: getTextTabFileName(tab),
        lockable: canLockTab(tab)
      }
    case 'diff':
      if (tab.kind !== 'diff') {
        throw new Error('Invalid diff tab presentation request')
      }
      return {
        surfaceKind,
        title: getDiffTabTitle(tab),
        preview: isSurfacePreview(tab),
        providerIcon: null,
        fileName: null,
        lockable: false
      }
    case 'tool':
      if (tab.kind !== 'tool') throw new Error('Invalid tool tab presentation request')
      return {
        surfaceKind,
        title: getToolTabTitle(tab),
        preview: isSurfacePreview(tab),
        providerIcon: null,
        fileName: null,
        lockable: false
      }
    default: {
      const _exhaustive: never = surfaceKind
      return _exhaustive
    }
  }
}
