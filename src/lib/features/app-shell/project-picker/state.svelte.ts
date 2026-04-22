let projectPickerOverride = $state(false)

export function isProjectPickerOverride(): boolean {
  return projectPickerOverride
}

export function showProjectPicker() {
  projectPickerOverride = true
}

export function hideProjectPicker() {
  projectPickerOverride = false
}
