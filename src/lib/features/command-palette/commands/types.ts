import type { Component } from 'svelte'

export interface Command {
  id: string
  label: string
  subtitle?: string
  icon?: Component
  iconSrc?: string
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
