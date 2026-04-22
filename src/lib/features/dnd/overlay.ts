export type Zone = 'merge' | 'up' | 'down' | 'left' | 'right'

export interface ZoneGeom {
  top: string
  left: string
  width: string
  height: string
}

export interface ComputeZoneOptions {
  edgeMarginRatio?: number
  allowSplit?: boolean
}

export function computeZone(
  localX: number,
  localY: number,
  width: number,
  height: number,
  options?: ComputeZoneOptions
): Zone {
  if (!options?.allowSplit) return 'merge'
  if (width <= 0 || height <= 0) return 'merge'

  const edgeMarginRatio = clampMarginRatio(options.edgeMarginRatio ?? 0.1)
  const edgeX = edgeMarginRatio * width
  const edgeY = edgeMarginRatio * height

  const inCenter = localX > edgeX && localX < width - edgeX && localY > edgeY && localY < height - edgeY

  if (inCenter) return 'merge'

  const dLeft = localX / width
  const dRight = 1 - dLeft
  const dUp = localY / height
  const dDown = 1 - dUp
  const min = Math.min(dLeft, dRight, dUp, dDown)

  if (min === dLeft) return 'left'
  if (min === dRight) return 'right'
  if (min === dUp) return 'up'
  return 'down'
}

export function zoneGeometry(zone: Zone): ZoneGeom {
  switch (zone) {
    case 'up':
      return { top: '0', left: '0', width: '100%', height: '50%' }
    case 'down':
      return { top: '50%', left: '0', width: '100%', height: '50%' }
    case 'left':
      return { top: '0', left: '0', width: '50%', height: '100%' }
    case 'right':
      return { top: '0', left: '50%', width: '50%', height: '100%' }
    case 'merge':
      return { top: '0', left: '0', width: '100%', height: '100%' }
  }
}

function clampMarginRatio(value: number): number {
  if (!Number.isFinite(value)) return 0.1
  return Math.min(0.45, Math.max(0.01, value))
}
