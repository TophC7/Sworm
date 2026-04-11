/**
 * Drag-handle panel resize as an {@attach} action.
 *
 * Usage:
 *   const resize = createPanelResize(240, 200, 420);
 *   <Panel style={`width: ${resize.width}px`} />
 *   <div {@attach resize.handle((e) => e.clientX)} />
 */

function clamp(value: number, min: number, max: number): number {
	return Math.min(Math.max(value, min), max);
}

/**
 * Create reactive resize state for a panel drag handle.
 *
 * Returns a reactive `width` getter/setter and a `handle()` method
 * that produces an {@attach}-compatible function for the drag handle element.
 *
 * @param toWidth - converts a PointerEvent to the desired panel width.
 *   Passed to `handle()` so each usage can compute width differently
 *   (e.g. `e.clientX` vs `container.right - e.clientX`).
 */
export function createPanelResize(initial: number, min: number, max: number) {
	let width = $state(initial);

	/**
	 * Returns an {@attach}-compatible function that wires pointer
	 * tracking on the given element.
	 */
	function handle(toWidth: (e: PointerEvent) => number) {
		return (element: HTMLElement) => {
			function onPointerDown(e: PointerEvent) {
				e.preventDefault();
				document.body.style.cursor = 'col-resize';
				document.body.style.userSelect = 'none';

				function onMove(e: PointerEvent) {
					width = clamp(toWidth(e), min, max);
				}
				function onUp() {
					document.body.style.cursor = '';
					document.body.style.userSelect = '';
					window.removeEventListener('pointermove', onMove);
					window.removeEventListener('pointerup', onUp);
				}

				window.addEventListener('pointermove', onMove);
				window.addEventListener('pointerup', onUp);
			}

			element.addEventListener('pointerdown', onPointerDown);
			return () => element.removeEventListener('pointerdown', onPointerDown);
		};
	}

	return {
		get width() {
			return width;
		},
		set width(v: number) {
			width = v;
		},
		handle
	};
}
