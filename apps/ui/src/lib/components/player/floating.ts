/** Where the floating player sits: offsets (px) from the nearest edges. */
export interface FloatPos {
  h: 'left' | 'right';
  v: 'top' | 'bottom';
  x: number;
  y: number;
}

/** The window area the player lives in (above the bottom inset) and its size. */
export interface FloatBox {
  w: number;
  h: number;
  /** Area width and height. */
  W: number;
  H: number;
  /** Smallest top offset (keeps clear of e.g. the phone drawer toggle). */
  minTop: number;
}

export const SNAP = 24;

const clamp = (n: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, n));

/** Position from a top-left corner, anchored to the nearest sides and clamped on screen. */
export function place(left: number, top: number, b: FloatBox): FloatPos {
  const l = clamp(left, 0, Math.max(0, b.W - b.w));
  const t = clamp(top, b.minTop, Math.max(b.minTop, b.H - b.h));
  const h = l + b.w / 2 < b.W / 2 ? 'left' : 'right';
  const v = t + b.h / 2 < b.H / 2 ? 'top' : 'bottom';
  return {
    h,
    v,
    x: h === 'left' ? l : b.W - b.w - l,
    y: v === 'top' ? t : b.H - b.h - t,
  };
}

/** Top-left corner of a position, clamped so it stays on screen after a resize. */
export function resolve(p: FloatPos, b: FloatBox): { left: number; top: number } {
  const q = place(p.h === 'left' ? p.x : b.W - b.w - p.x, p.v === 'top' ? p.y : b.H - b.h - p.y, b);
  return {
    left: q.h === 'left' ? q.x : b.W - b.w - q.x,
    top: q.v === 'top' ? q.y : b.H - b.h - q.y,
  };
}

/** Magnetic edges: an offset within SNAP of its edge goes flush to it. */
export function snap(p: FloatPos, b: FloatBox): FloatPos {
  return {
    ...p,
    x: p.x <= SNAP ? 0 : p.x,
    y: p.v === 'top' ? (p.y <= b.minTop + SNAP ? b.minTop : p.y) : p.y <= SNAP ? 0 : p.y,
  };
}

/** Whether a stored value is a position in the current format (old ones were top-left). */
export function isFloatPos(v: unknown): v is FloatPos {
  const p = v as FloatPos | null;
  return (
    !!p &&
    (p.h === 'left' || p.h === 'right') &&
    (p.v === 'top' || p.v === 'bottom') &&
    Number.isFinite(p.x) &&
    Number.isFinite(p.y)
  );
}
