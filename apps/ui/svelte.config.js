import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

import { hoverGatePreprocess } from './hoverGate.js';

export default {
  preprocess: [vitePreprocess(), hoverGatePreprocess],
};
