import details from './presets/details.kdl?raw';
import gallery from './presets/gallery.kdl?raw';
import tracksArt from './presets/tracks-art.kdl?raw';
import tracksGallery from './presets/tracks-gallery.kdl?raw';
import tracks from './presets/tracks.kdl?raw';

export interface LayoutSource {
  /** `builtin:<file>` or `user:<file>`. */
  id: string;
  fileName: string;
  text: string;
  builtin: boolean;
}

/** Shipped with the app, in button order. The first is the default. */
export const builtinLayouts: LayoutSource[] = [
  ['tracks.kdl', tracks],
  ['tracks-art.kdl', tracksArt],
  ['details.kdl', details],
  ['tracks-gallery.kdl', tracksGallery],
  ['gallery.kdl', gallery],
].map(([fileName, text]) => ({ id: `builtin:${fileName}`, fileName, text, builtin: true }));
