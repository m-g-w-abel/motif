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
  /** Smallest top offset (keeps clear of e.g. the phone back button). */
  minTop: number;
}

export const SNAP = 24;
/** Gap kept between the player and the window edges. */
export const MARGIN = 16;

const clamp = (n: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, n));

/** Smallest top offset: the margin, or more where something sits at the top. */
const topFloor = (b: FloatBox) => Math.max(b.minTop, MARGIN);

/** Position from a top-left corner, anchored to the nearest sides and kept MARGIN inside the window. */
export function place(left: number, top: number, b: FloatBox): FloatPos {
  const l = clamp(left, MARGIN, Math.max(MARGIN, b.W - b.w - MARGIN));
  const t = clamp(top, topFloor(b), Math.max(topFloor(b), b.H - b.h - MARGIN));
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

/** Magnetic edges: within SNAP of the margin, the player settles on it. */
export function snap(p: FloatPos, b: FloatBox): FloatPos {
  const floor = p.v === 'top' ? topFloor(b) : MARGIN;
  return {
    ...p,
    x: p.x <= MARGIN + SNAP ? MARGIN : p.x,
    y: p.y <= floor + SNAP ? floor : p.y,
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
