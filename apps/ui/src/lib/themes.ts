/**
 * Colour themes. Each is a set of values for the CSS tokens in app.css, so
 * user theme files (later) only have to supply the same keys. The accent
 * colour is a separate setting and works with every theme.
 */

export interface Theme {
  id: string;
  name: string;
  scheme: 'light' | 'dark';
  tokens: {
    bg: string;
    surface: string;
    'surface-2': string;
    hover: string;
    border: string;
    /** Stronger line between the app's main regions. */
    divider: string;
    text: string;
    'text-muted': string;
    shadow: string;
  };
}

const darkShadow = '0 8px 32px rgb(0 0 0 / 0.5)';
const lightShadow = '0 6px 24px rgb(0 0 0 / 0.18)';

export const THEMES: Theme[] = [
  {
    id: 'graphite',
    name: 'Graphite',
    scheme: 'dark',
    tokens: {
      bg: '#121214',
      surface: '#19191c',
      'surface-2': '#222226',
      hover: 'rgb(255 255 255 / 0.05)',
      border: '#2b2b30',
      divider: '#45454d',
      text: '#e7e7ea',
      'text-muted': '#8b8b94',
      shadow: darkShadow,
    },
  },
  {
    id: 'midnight',
    name: 'Midnight',
    scheme: 'dark',
    tokens: {
      bg: '#000000',
      surface: '#0b0b0d',
      'surface-2': '#151518',
      hover: 'rgb(255 255 255 / 0.06)',
      border: '#1f1f23',
      divider: '#3a3a41',
      text: '#ededf0',
      'text-muted': '#85858e',
      shadow: darkShadow,
    },
  },
  {
    id: 'nord',
    name: 'Nord',
    scheme: 'dark',
    tokens: {
      bg: '#2b303b',
      surface: '#323846',
      'surface-2': '#3b4252',
      hover: 'rgb(255 255 255 / 0.05)',
      border: '#434c5e',
      divider: '#58627a',
      text: '#eceff4',
      'text-muted': '#9aa3b5',
      shadow: darkShadow,
    },
  },
  {
    id: 'light',
    name: 'Light',
    scheme: 'light',
    tokens: {
      bg: '#f4f4f5',
      surface: '#ffffff',
      'surface-2': '#e9e9ec',
      hover: 'rgb(0 0 0 / 0.05)',
      border: '#dcdce1',
      divider: '#b9b9c2',
      text: '#18181b',
      'text-muted': '#6c6c75',
      shadow: lightShadow,
    },
  },
  {
    id: 'paper',
    name: 'Paper',
    scheme: 'light',
    tokens: {
      bg: '#f5f1e8',
      surface: '#fbf8f1',
      'surface-2': '#ece6d8',
      hover: 'rgb(60 40 0 / 0.06)',
      border: '#e0d8c6',
      divider: '#c3b89f',
      text: '#2b2620',
      'text-muted': '#7a7062',
      shadow: lightShadow,
    },
  },
  {
    id: 'mist',
    name: 'Mist',
    scheme: 'light',
    tokens: {
      bg: '#eef0f5',
      surface: '#f8f9fc',
      'surface-2': '#e2e6ee',
      hover: 'rgb(20 30 60 / 0.05)',
      border: '#d6dbe6',
      divider: '#aeb6c8',
      text: '#1b2030',
      'text-muted': '#677089',
      shadow: lightShadow,
    },
  },
];

/** Used for "System" when the OS is in light or dark mode. */
export const SYSTEM_THEMES = { light: 'light', dark: 'graphite' } as const;

export function themeById(id: string): Theme {
  return THEMES.find((t) => t.id === id) ?? THEMES[0];
}
