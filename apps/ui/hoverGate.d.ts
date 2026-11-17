import type { Plugin } from 'vite';
export function gateHover(css: string): string;
export const hoverGatePreprocess: { style: (input: { content: string }) => { code: string } };
export const hoverGatePlugin: Plugin;
