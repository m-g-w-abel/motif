import { describe, expect, it } from 'vitest';
import { gateHover } from './hoverGate.js';

const squash = (s: string) => s.replace(/\s+/g, '');

describe('gateHover', () => {
  it('wraps rules that use :hover and leaves others', () => {
    expect(squash(gateHover('a { x: 1 } b:hover:not(:disabled) { y: 2 } d { z: 3 }'))).toBe('a{x:1}@media(hover:hover){b:hover:not(:disabled){y:2}}d{z:3}');
  });

  it('gates only the selectors that use :hover, so the rest still apply on touch', () => {
    expect(squash(gateHover('.picked, .rows .picked:hover { a: b } button:hover, button:focus-visible { c: d }'))).toBe(
      '.picked{a:b}@media(hover:hover){.rows.picked:hover{a:b}}button:focus-visible{c:d}@media(hover:hover){button:hover{c:d}}',
    );
  });

  it('keeps commas inside parentheses in their selector', () => {
    expect(squash(gateHover('.hot, .slot:is(:hover, :focus) { a: b }'))).toBe('.hot{a:b}@media(hover:hover){.slot:is(:hover,:focus){a:b}}');
  });

  it('gates inside @media, and leaves @keyframes alone', () => {
    const out = squash(gateHover('@media (max-width: 700px) { li:hover { a: b } } @keyframes k { from { a: 1 } }'));
    expect(out).toBe('@media(max-width:700px){@media(hover:hover){li:hover{a:b}}}@keyframesk{from{a:1}}');
  });

  it('gates rules that follow an at-rule it does not descend into', () => {
    expect(squash(gateHover('@keyframes k { from { a: 1 } } b:hover { c: d }'))).toBe(
      squash('@keyframes k { from { a: 1 } } @media (hover: hover) { b:hover { c: d }}'),
    );
  });

  it('ignores :hover in comments', () => {
    expect(squash(gateHover('/* a:hover */ b { c: d }'))).toBe('b{c:d}');
  });
});
