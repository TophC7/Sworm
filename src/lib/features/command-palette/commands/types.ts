import type { Component } from 'svelte'

export interface CommandConfirm {
  title: string
  message: string
  confirmLabel: string
  isOpen: () => boolean
  onConfirm: () => void
  onCancel: () => void
}

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
  onSelect: () => void
  confirm?: CommandConfirm
}

export interface CommandGroup {
  heading: string
  commands: Command[]
}

export interface FileCallbacks {
  onNewProject: () => void
  onSettings: () => void
}
