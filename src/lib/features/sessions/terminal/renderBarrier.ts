export class RenderBarrier {
  received = 0
  rendered = 0
  private readonly outOfOrder = new Set<number>()
  private readonly waiters = new Set<() => void>()

  next(): number {
    return ++this.received
  }

  seed(seq: number): void {
    this.received = seq
    this.rendered = seq
    this.outOfOrder.clear()
  }

  reset(): void {
    this.seed(0)
  }

  markRendered(seq: number): void {
    if (seq <= this.rendered) return
    this.outOfOrder.add(seq)
    while (this.outOfOrder.has(this.rendered + 1)) {
      this.rendered++
      this.outOfOrder.delete(this.rendered)
    }
    for (const waiter of Array.from(this.waiters)) waiter()
  }

  waitFor(target: number): Promise<void> {
    if (this.rendered >= target) return Promise.resolve()
    return new Promise<void>((resolve) => {
      const check = () => {
        if (this.rendered >= target) {
          this.waiters.delete(check)
          resolve()
        }
      }
      this.waiters.add(check)
    })
  }
}
