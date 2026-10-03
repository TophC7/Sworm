import type { Component } from 'svelte'
import type { ServerState } from '$lib/features/browser/places.svelte'

export interface Command {
  id: string
  label: string
  subtitle?: string
  icon?: Component
  iconSrc?: string
  serverState?: ServerState
  /**
   * Kebab-case Lucide icon name rendered via the shared LucideIcon
   * component. Lets entries defined by user config (e.g. tasks) ship
   * an arbitrary icon without the author bundling a Svelte component.
   */
  lucideIcon?: string
  keywords: string[]
  shortcut?: string
  defaultKeybindings?: string[]
  onSelect: () => void
}

export interface CommandGroup {
  heading: string
  commands: Command[]
}
