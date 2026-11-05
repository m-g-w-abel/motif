export interface Toast {
  id: number;
  message: string;
  kind: 'info' | 'error';
}

/** Short-lived notifications, e.g. "Installed layout X". */
class Toasts {
  items = $state<Toast[]>([]);
  #nextId = 1;

  show(message: string, kind: Toast['kind'] = 'info') {
    const id = this.#nextId++;
    this.items = [...this.items, { id, message, kind }];
    setTimeout(() => this.dismiss(id), kind === 'error' ? 8000 : 3000);
    return id;
  }

  dismiss(id: number) {
    this.items = this.items.filter((t) => t.id !== id);
  }
}

export const toasts = new Toasts();
