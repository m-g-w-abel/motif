/**
 * The scroll offset for a thumb dragged `dy` screen pixels from where it was
 * grabbed (`grabbed`: its offset then, in CSS pixels). The root's `zoom`
 * scales CSS pixels on screen; `track` and `size` are the bar's and thumb's
 * heights and `max` the scroll range now, which change while rows load.
 */
export function dragScroll(grabbed: number, dy: number, zoom: number, track: number, size: number, max: number): number {
  const travel = Math.max(1, track - size);
  const thumb = Math.min(travel, Math.max(0, grabbed + dy / zoom));
  return (thumb / travel) * max;
}
