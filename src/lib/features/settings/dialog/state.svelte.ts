let settingsOpen = $state(false)

export function isSettingsOpen(): boolean {
  return settingsOpen
}

export function setSettingsOpen(open: boolean) {
  settingsOpen = open
}

export function toggleSettings() {
  settingsOpen = !settingsOpen
}
