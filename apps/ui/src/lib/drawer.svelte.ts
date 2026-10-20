/**
 * The left panel on phones: hidden off-screen, opened by the menu button or a
 * swipe in from the left edge, closed by swiping it back or tapping outside.
 */
export class Drawer {
  open = $state(false);

  toggle() {
    this.open = !this.open;
  }

  close() {
    this.open = false;
  }

  /**
   * Starts tracking a horizontal swipe. From the screen edge, a swipe right
   * opens the drawer; on the open drawer, a swipe left closes it.
   */
  swipe(e: PointerEvent) {
    const fromEdge = !this.open && e.clientX <= 24;
    if (!fromEdge && !this.open) return;
    const startX = e.clientX;
    const startY = e.clientY;

    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - startX;
      // Mostly-vertical movement is a scroll, not a swipe.
      if (Math.abs(ev.clientY - startY) > Math.abs(dx)) return end();
      if (fromEdge && dx > 50) {
        this.open = true;
        end();
      } else if (!fromEdge && dx < -50) {
        this.open = false;
        end();
      }
    };
    const end = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', end);
      window.removeEventListener('pointercancel', end);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', end);
    window.addEventListener('pointercancel', end);
  }
}
