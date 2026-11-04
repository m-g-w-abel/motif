<script lang="ts">
  import App from './App.svelte';
  import { engines } from './lib/engine.svelte';
</script>

<!-- Switching engines starts the app over, so library, artwork and queue all come from the new one. -->
{#key engines.list.current}<App />{/key}
