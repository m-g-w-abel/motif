// Touch screens report :hover for whatever the finger last touched, so rows and
// tiles look selected after a scroll. Every selector that uses :hover is
// wrapped in @media (hover: hover), so only real pointers get hover styles.
// Used for component styles (Svelte preprocessor) and app.css (Vite plugin).
export function gateHover(css) {
  const src = css.replace(/\/\*[\s\S]*?\*\//g, '');
  let out = '';
  let start = 0;
  for (let i = 0; i < src.length; i++) {
    if (src[i] !== '{') continue;
    const prelude = src.slice(start, i);
    const atRule = prelude.trimStart().startsWith('@');
    let depth = 1;
    let j = i + 1;
    while (depth && j < src.length) depth += src[j] === '{' ? 1 : src[j] === '}' ? -1 : 0, j++;
    if (atRule) {
      // Descend into @media/@supports; leave @keyframes and the like alone.
      if (/^\s*@(media|supports|layer)/.test(prelude)) {
        out += src.slice(start, i + 1) + gateHover(src.slice(i + 1, j - 1)) + '}';
        i = j - 1;
        start = j;
      } else {
        out += src.slice(start, j);
        i = j - 1;
        start = j;
      }
      continue;
    }
    out += gate(prelude, src.slice(i, j));
    i = j - 1;
    start = j;
  }
  return out + src.slice(start);
}

/** The rule `selectors` + `block`, with only its selectors that use :hover gated. */
function gate(selectors, block) {
  if (!selectors.includes(':hover')) return selectors + block;
  const parts = topLevel(selectors);
  const rest = parts.filter((p) => !p.includes(':hover'));
  const hover = parts.filter((p) => p.includes(':hover'));
  return (rest.length ? rest.join(',') + block : '') + `@media (hover: hover) {${hover.join(',')}${block}}`;
}

/** `selectors` split at the commas between them, not those inside `:is(…)` and the like. */
function topLevel(selectors) {
  const parts = [];
  let depth = 0;
  let from = 0;
  for (let k = 0; k < selectors.length; k++) {
    if (selectors[k] === '(') depth++;
    else if (selectors[k] === ')') depth--;
    else if (selectors[k] === ',' && !depth) {
      parts.push(selectors.slice(from, k));
      from = k + 1;
    }
  }
  return [...parts, selectors.slice(from)];
}

export const hoverGatePreprocess = { style: ({ content }) => ({ code: gateHover(content) }) };

export const hoverGatePlugin = {
  name: 'hover-gate',
  enforce: 'pre',
  transform: (code, id) => (/\.css$/.test(id.split('?')[0]) && !id.includes('node_modules') ? { code: gateHover(code), map: null } : null),
};
