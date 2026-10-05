export function delayedDragHover(delayMs: number, onTrigger: () => void) {
  return (element: HTMLElement) => {
    let timer: number | null = null

    const onCancel = () => {
      if (timer === null) return
      window.clearTimeout(timer)
      timer = null
    }

    const onDragOver = (event: DragEvent) => {
      event.preventDefault()
      if (timer !== null) return
      timer = window.setTimeout(() => {
        timer = null
        onTrigger()
      }, delayMs)
    }

    element.addEventListener('dragover', onDragOver)
    element.addEventListener('dragleave', onCancel)
    element.addEventListener('drop', onCancel)
    element.addEventListener('dragend', onCancel)

    return () => {
      onCancel()
      element.removeEventListener('dragover', onDragOver)
      element.removeEventListener('dragleave', onCancel)
      element.removeEventListener('drop', onCancel)
      element.removeEventListener('dragend', onCancel)
    }
  }
}
