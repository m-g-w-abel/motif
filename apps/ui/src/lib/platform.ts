/** The device running this UI is an Android phone (the app or a browser): no drag and drop, no file manager. */
export const isAndroid = /Android/i.test(navigator.userAgent);

/**
 * The file input filter for layout and theme files. Android knows no type for
 * `.kdl`, and the app's file chooser fails on a filter it can't map, so the
 * phone shows every file (the installers reject what isn't a layout or theme).
 */
export const kdlAccept = isAndroid ? undefined : '.kdl';
