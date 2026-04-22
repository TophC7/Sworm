interface DelayedHoverController {
  arm: () => void
  cancel: () => void
}

export function createDelayedHover(delayMs: number, onTrigger: () => void): DelayedHoverController {
  let timer: number | null = null

  const cancel = () => {
    if (timer === null) return
    window.clearTimeout(timer)
    timer = null
  }

  const arm = () => {
    if (timer !== null) return
    timer = window.setTimeout(() => {
      timer = null
      onTrigger()
    }, delayMs)
  }

  return { arm, cancel }
}

export function delayedDragHover(delayMs: number, onTrigger: () => void) {
  return (element: HTMLElement) => {
    const controller = createDelayedHover(delayMs, onTrigger)

    const onDragOver = (event: DragEvent) => {
      event.preventDefault()
      controller.arm()
    }
    const onCancel = () => {
      controller.cancel()
    }

    element.addEventListener('dragover', onDragOver)
    element.addEventListener('dragleave', onCancel)
    element.addEventListener('drop', onCancel)
    element.addEventListener('dragend', onCancel)

    return () => {
      controller.cancel()
      element.removeEventListener('dragover', onDragOver)
      element.removeEventListener('dragleave', onCancel)
      element.removeEventListener('drop', onCancel)
      element.removeEventListener('dragend', onCancel)
    }
  }
}
