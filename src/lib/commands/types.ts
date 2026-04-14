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
