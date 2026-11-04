import { mount } from 'svelte';
import Root from './Root.svelte';
import PairPage from './lib/components/pairing/PairPage.svelte';
import './app.css';

// A new device opens the host's pairing QR code at /pair.
const target = document.getElementById('app')!;
export default location.pathname === '/pair' ? mount(PairPage, { target }) : mount(Root, { target });
