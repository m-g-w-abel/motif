/**
 * The expanded now-playing sheet, which grows out of the player bar.
 * `progress` runs from 0 (collapsed) to 1 (covering the screen); dragging
 * sets it directly, and releasing snaps it to either end.
 */
export class PlayerSheet {
  progress = $state(0);
  /** True while a finger or pointer is moving the sheet, to disable easing. */
  dragging = $state(false);

  get open() {
    return this.progress > 0;
  }

  expand() {
    this.dragging = false;
    this.progress = 1;
  }

  collapse() {
    this.dragging = false;
    this.progress = 0;
  }

  /**
   * Tracks a vertical drag. `direction` is the way that opens the sheet:
   * -1 when dragging up opens it (from the bar), 1 when dragging down closes
   * it (from the sheet). A tap without movement calls `onTap`.
   */
  drag(e: PointerEvent, from: 'bar' | 'sheet', onTap?: () => void) {
    if (e.button !== 0) return;
    const startY = e.clientY;
    const startProgress = this.progress;
    const height = window.innerHeight;
    let lastY = startY;
    let lastT = performance.now();
    let velocity = 0;
    let moved = false;

    const move = (ev: PointerEvent) => {
      const dy = ev.clientY - startY;
      if (!moved && Math.abs(dy) < 8) return;
      moved = true;
      this.dragging = true;
      this.progress = Math.min(1, Math.max(0, startProgress - dy / height));
      const now = performance.now();
      velocity = (ev.clientY - lastY) / Math.max(1, now - lastT);
      lastY = ev.clientY;
      lastT = now;
    };

    const end = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', end);
      window.removeEventListener('pointercancel', end);
      if (!moved) {
        onTap?.();
        return;
      }
      // A flick decides by direction; a slow drag by how far it got.
      const flick = Math.abs(velocity) > 0.5;
      const open = flick ? velocity < 0 : this.progress > (from === 'bar' ? 0.25 : 0.75);
      if (open) this.expand();
      else this.collapse();
    };

    // On the window, so a fast flick that leaves the bar still counts.
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', end);
    window.addEventListener('pointercancel', end);
  }
}
