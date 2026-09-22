let settingsOpen = $state(false)
let settingsPage = $state('appearance')

export function getSettingsPage(): string {
  return settingsPage
}

export function setSettingsPage(page: string): void {
  settingsPage = page
}

export function isSettingsOpen(): boolean {
  return settingsOpen
}

export function setSettingsOpen(open: boolean) {
  settingsOpen = open
}

export function toggleSettings() {
  settingsOpen = !settingsOpen
}
