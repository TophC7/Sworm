export interface XtermWriter {
  write(data: Uint8Array, callback?: () => void): void
}

type Schedule = (callback: () => void) => void

interface PendingWrite {
  data: Uint8Array
  offset: number
  resolve: () => void
  reject: (error: unknown) => void
}

const DEFAULT_CHUNK_BYTES = 16 * 1024

/**
 * Feeds xterm in bounded parser slices with at most one write in flight.
 *
 * xterm's own write buffer becomes unresponsive when producers outrun
 * its parser. Keeping that buffer shallow also gives WebKit a task boundary
 * between slices so keyboard events and rendering are not starved by a
 * large transcript replay or PTY burst.
 */
export class XtermWriteQueue {
  private readonly pending: PendingWrite[] = []
  private active = false
  private disposed = false

  constructor(
    private readonly writer: XtermWriter,
    private readonly chunkBytes = DEFAULT_CHUNK_BYTES,
    private readonly schedule: Schedule = (callback) => window.setTimeout(callback, 0)
  ) {
    if (chunkBytes < 1) {
      throw new RangeError('chunkBytes must be positive')
    }
  }

  /** The caller must not mutate `data` until the returned promise settles. */
  write(data: Uint8Array): Promise<void> {
    if (this.disposed || data.byteLength === 0) {
      return Promise.resolve()
    }

    return new Promise<void>((resolve, reject) => {
      this.pending.push({ data, offset: 0, resolve, reject })
      this.pump()
    })
  }

  dispose(): void {
    if (this.disposed) return
    this.disposed = true
    for (const write of this.pending) write.resolve()
    this.pending.length = 0
  }

  private pump(): void {
    if (this.disposed || this.active) return

    const write = this.pending[0]
    if (!write) return

    const end = Math.min(write.offset + this.chunkBytes, write.data.byteLength)
    const chunk = write.data.subarray(write.offset, end)
    this.active = true

    try {
      this.writer.write(chunk, () => {
        this.active = false
        if (this.disposed) return

        write.offset = end
        if (write.offset >= write.data.byteLength) {
          this.pending.shift()
          write.resolve()
        }
        this.schedule(() => this.pump())
      })
    } catch (error) {
      this.active = false
      this.pending.shift()
      write.reject(error)
      this.schedule(() => this.pump())
    }
  }
}
