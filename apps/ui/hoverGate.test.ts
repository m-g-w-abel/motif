import { describe, expect, it } from 'vitest';
import { gateHover } from './hoverGate.js';

const squash = (s: string) => s.replace(/\s+/g, '');

describe('gateHover', () => {
  it('wraps rules that use :hover and leaves others', () => {
    expect(squash(gateHover('a { x: 1 } b:hover:not(:disabled), c { y: 2 } d { z: 3 }'))).toBe('a{x:1}@media(hover:hover){b:hover:not(:disabled),c{y:2}}d{z:3}');
  });

  it('gates inside @media, and leaves @keyframes alone', () => {
    const out = squash(gateHover('@media (max-width: 700px) { li:hover { a: b } } @keyframes k { from { a: 1 } }'));
    expect(out).toBe('@media(max-width:700px){@media(hover:hover){li:hover{a:b}}}@keyframesk{from{a:1}}');
  });

  it('ignores :hover in comments', () => {
    expect(squash(gateHover('/* a:hover */ b { c: d }'))).toBe('b{c:d}');
  });
});
