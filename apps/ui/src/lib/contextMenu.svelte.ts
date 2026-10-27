export interface MenuItem {
  label: string;
  run: () => void;
}

/** The one open context menu, drawn by `ContextMenu.svelte`. */
class ContextMenuState {
  items = $state<MenuItem[]>([]);
  x = $state(0);
  y = $state(0);
  /** Gets the focus back when the menu closes. */
  #origin: HTMLElement | null = null;

  show(items: MenuItem[], x: number, y: number, origin: HTMLElement | null) {
    this.items = items;
    this.x = x;
    this.y = y;
    this.#origin = origin;
  }

  close(restoreFocus = false) {
    if (!this.items.length) return;
    this.items = [];
    if (restoreFocus) this.#origin?.focus({ preventScroll: true });
  }
}

export const contextMenu = new ContextMenuState();

/**
 * Svelte action: right-click, the Menu key or Shift+F10 on the element opens
 * its menu. Touch long-press arrives as a `contextmenu` event too.
 */
export function menu(node: HTMLElement, items: () => MenuItem[]) {
  const open = (x: number, y: number) => {
    const list = items();
    if (list.length) contextMenu.show(list, x, y, node);
  };
  const onContextMenu = (e: MouseEvent) => {
    e.preventDefault();
    if (e.clientX || e.clientY) open(e.clientX, e.clientY);
    else open(node.getBoundingClientRect().left + 16, node.getBoundingClientRect().top + 16);
  };
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.target !== node || !(e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey))) return;
    e.preventDefault();
    const box = node.getBoundingClientRect();
    open(box.left + 16, box.top + box.height / 2);
  };
  node.addEventListener('contextmenu', onContextMenu);
  node.addEventListener('keydown', onKeyDown);
  return {
    update(next: () => MenuItem[]) {
      items = next;
    },
    destroy() {
      node.removeEventListener('contextmenu', onContextMenu);
      node.removeEventListener('keydown', onKeyDown);
    },
  };
}
