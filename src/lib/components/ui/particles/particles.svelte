<script lang="ts">
  import { cn } from '$lib/utils/cn'

  let {
    class: className,
    quantity = 80,
    staticity = 50,
    ease = 50,
    size = 0.4,
    // Mirrors --color-accent (see src/app.css). Canvas fillStyle needs a
    // literal hex; keep this default in sync if the accent token moves.
    color = '#ffb59f',
    vx = 0,
    vy = 0
  }: {
    class?: string
    quantity?: number
    staticity?: number
    ease?: number
    size?: number
    color?: string
    vx?: number
    vy?: number
  } = $props()

  let canvasRef = $state<HTMLCanvasElement | null>(null)
  let containerRef = $state<HTMLDivElement | null>(null)

  interface Circle {
    x: number
    y: number
    translateX: number
    translateY: number
    size: number
    alpha: number
    targetAlpha: number
    dx: number
    dy: number
    magnetism: number
  }

  let context: CanvasRenderingContext2D | null = null
  let circles: Circle[] = []
  let mouse = { x: 0, y: 0 }
  let canvasSize = { w: 0, h: 0 }
  let animId: number | null = null
  const dpr = typeof window !== 'undefined' ? window.devicePixelRatio : 1

  function hexToRgb(hex: string): [number, number, number] {
    hex = hex.replace('#', '')
    if (hex.length === 3) {
      hex = hex
        .split('')
        .map((c) => c + c)
        .join('')
    }
    const n = parseInt(hex, 16)
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255]
  }

  function circleParams(): Circle {
    return {
      x: Math.floor(Math.random() * canvasSize.w),
      y: Math.floor(Math.random() * canvasSize.h),
      translateX: 0,
      translateY: 0,
      size: Math.floor(Math.random() * 2) + size,
      alpha: 0,
      targetAlpha: parseFloat((Math.random() * 0.6 + 0.1).toFixed(1)),
      dx: (Math.random() - 0.5) * 0.1,
      dy: (Math.random() - 0.5) * 0.1,
      magnetism: 0.1 + Math.random() * 4
    }
  }

  function resizeCanvas() {
    if (!containerRef || !canvasRef || !context) return
    circles.length = 0
    canvasSize.w = containerRef.offsetWidth
    canvasSize.h = containerRef.offsetHeight
    canvasRef.width = canvasSize.w * dpr
    canvasRef.height = canvasSize.h * dpr
    canvasRef.style.width = `${canvasSize.w}px`
    canvasRef.style.height = `${canvasSize.h}px`
    context.scale(dpr, dpr)
    drawParticles()
  }

  function drawCircle(circle: Circle, update = false) {
    if (!context) return
    const { x, y, translateX, translateY, size: s, alpha } = circle
    context.translate(translateX, translateY)
    context.beginPath()
    context.arc(x, y, s, 0, 2 * Math.PI)
    context.fillStyle = `rgba(${rgb.join(', ')}, ${alpha})`
    context.fill()
    context.setTransform(dpr, 0, 0, dpr, 0, 0)
    if (!update) circles.push(circle)
  }

  function drawParticles() {
    if (!context) return
    context.clearRect(0, 0, canvasSize.w, canvasSize.h)
    for (let i = 0; i < quantity; i++) {
      drawCircle(circleParams())
    }
  }

  function remapValue(value: number, s1: number, e1: number, s2: number, e2: number): number {
    const r = ((value - s1) * (e2 - s2)) / (e1 - s1) + s2
    return r > 0 ? r : 0
  }

  function animate() {
    if (!context) return
    context.clearRect(0, 0, canvasSize.w, canvasSize.h)
    for (let i = circles.length - 1; i >= 0; i--) {
      const c = circles[i]
      const edge = [
        c.x + c.translateX - c.size,
        canvasSize.w - c.x - c.translateX - c.size,
        c.y + c.translateY - c.size,
        canvasSize.h - c.y - c.translateY - c.size
      ]
      const closest = Math.min(...edge)
      const remap = parseFloat(remapValue(closest, 0, 20, 0, 1).toFixed(2))
      if (remap > 1) {
        c.alpha += 0.02
        if (c.alpha > c.targetAlpha) c.alpha = c.targetAlpha
      } else {
        c.alpha = c.targetAlpha * remap
      }
      c.x += c.dx + vx
      c.y += c.dy + vy
      c.translateX += (mouse.x / (staticity / c.magnetism) - c.translateX) / ease
      c.translateY += (mouse.y / (staticity / c.magnetism) - c.translateY) / ease
      drawCircle(c, true)
      if (c.x < -c.size || c.x > canvasSize.w + c.size || c.y < -c.size || c.y > canvasSize.h + c.size) {
        circles.splice(i, 1)
        drawCircle(circleParams())
      }
    }
    animId = requestAnimationFrame(animate)
  }

  function onMouseMove(event: MouseEvent) {
    if (!canvasRef) return
    const rect = canvasRef.getBoundingClientRect()
    const { w, h } = canvasSize
    const x = event.clientX - rect.left - w / 2
    const y = event.clientY - rect.top - h / 2
    if (x < w / 2 && x > -w / 2 && y < h / 2 && y > -h / 2) {
      mouse.x = x
      mouse.y = y
    }
  }

  let rgb = $derived(hexToRgb(color))

  $effect(() => {
    if (!canvasRef) return
    context = canvasRef.getContext('2d')
    resizeCanvas()
    animate()
    window.addEventListener('resize', resizeCanvas)
    window.addEventListener('mousemove', onMouseMove)
    return () => {
      if (animId !== null) cancelAnimationFrame(animId)
      window.removeEventListener('resize', resizeCanvas)
      window.removeEventListener('mousemove', onMouseMove)
    }
  })
</script>

<div class={cn('absolute inset-0', className)} bind:this={containerRef} aria-hidden="true">
  <canvas bind:this={canvasRef} class="h-full w-full"></canvas>
</div>
