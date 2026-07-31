// Terminal color scheme presets for xterm.js
// Each preset defines a complete ITheme: background, foreground, cursor,
// selectionHighlight, and all 16 ANSI colors (0-15).

import type { ITheme } from "@xterm/xterm";

export interface TerminalThemePreset {
  id: string;
  name: string;
  isDark: boolean;
  theme: ITheme;
}

// === SECTION 1 END ===

// Catppuccin Mocha — warm dark, the default
const catppuccinMocha: ITheme = {
  background: "#1e1e2e",
  foreground: "#cdd6f4",
  cursor: "#f5e0dc",
  selectionBackground: "#585b70",
  black: "#45475a",
  red: "#f38ba8",
  green: "#a6e3a1",
  yellow: "#f9e2af",
  blue: "#89b4fa",
  magenta: "#f5c2e7",
  cyan: "#94e2d5",
  white: "#bac2de",
  brightBlack: "#585b70",
  brightRed: "#f38ba8",
  brightGreen: "#a6e3a1",
  brightYellow: "#f9e2af",
  brightBlue: "#89b4fa",
  brightMagenta: "#f5c2e7",
  brightCyan: "#94e2d5",
  brightWhite: "#a6adc8",
};

// Catppuccin Latte — warm light
const catppuccinLatte: ITheme = {
  background: "#eff1f5",
  foreground: "#4c4f69",
  cursor: "#dc8a78",
  selectionBackground: "#bcc0cc",
  black: "#5c5f77",
  red: "#d20f39",
  green: "#40a02b",
  yellow: "#df8e1d",
  blue: "#1e66f5",
  magenta: "#ea76cb",
  cyan: "#179299",
  white: "#acB0be",
  brightBlack: "#6c6f85",
  brightRed: "#d20f39",
  brightGreen: "#40a02b",
  brightYellow: "#df8e1d",
  brightBlue: "#1e66f5",
  brightMagenta: "#ea76cb",
  brightCyan: "#179299",
  brightWhite: "#bcc0cc",
};

// === SECTION 2 END ===

// Dracula — classic dark purple
const dracula: ITheme = {
  background: "#282a36",
  foreground: "#f8f8f2",
  cursor: "#bd93f9",
  selectionBackground: "#44475a",
  black: "#21222c",
  red: "#ff5555",
  green: "#50fa7b",
  yellow: "#f1fa8c",
  blue: "#bd93f9",
  magenta: "#ff79c6",
  cyan: "#8be9fd",
  white: "#f8f8f2",
  brightBlack: "#6272a4",
  brightRed: "#ff6e6e",
  brightGreen: "#69ff94",
  brightYellow: "#ffffa5",
  brightBlue: "#d6acff",
  brightMagenta: "#ff92df",
  brightCyan: "#a4ffff",
  brightWhite: "#ffffff",
};

// One Dark — Atom's dark theme
const oneDark: ITheme = {
  background: "#282c34",
  foreground: "#abb2bf",
  cursor: "#528bff",
  selectionBackground: "#3e4451",
  black: "#282c34",
  red: "#e06c75",
  green: "#98c379",
  yellow: "#e5c07b",
  blue: "#61afef",
  magenta: "#c678dd",
  cyan: "#56b6c2",
  white: "#abb2bf",
  brightBlack: "#5c6370",
  brightRed: "#e06c75",
  brightGreen: "#98c379",
  brightYellow: "#e5c07b",
  brightBlue: "#61afef",
  brightMagenta: "#c678dd",
  brightCyan: "#56b6c2",
  brightWhite: "#ffffff",
};

// === SECTION 3 END ===

// Solarized Dark
const solarizedDark: ITheme = {
  background: "#002b36",
  foreground: "#839496",
  cursor: "#93a1a1",
  selectionBackground: "#073642",
  black: "#073642",
  red: "#dc322f",
  green: "#859900",
  yellow: "#b58900",
  blue: "#268bd2",
  magenta: "#d33682",
  cyan: "#2aa198",
  white: "#eee8d5",
  brightBlack: "#586e75",
  brightRed: "#cb4b16",
  brightGreen: "#586e75",
  brightYellow: "#657b83",
  brightBlue: "#839496",
  brightMagenta: "#6c71c4",
  brightCyan: "#93a1a1",
  brightWhite: "#fdf6e3",
};

// Solarized Light
const solarizedLight: ITheme = {
  background: "#fdf6e3",
  foreground: "#657b83",
  cursor: "#586e75",
  selectionBackground: "#eee8d5",
  black: "#073642",
  red: "#dc322f",
  green: "#859900",
  yellow: "#b58900",
  blue: "#268bd2",
  magenta: "#d33682",
  cyan: "#2aa198",
  white: "#eee8d5",
  brightBlack: "#586e75",
  brightRed: "#cb4b16",
  brightGreen: "#586e75",
  brightYellow: "#657b83",
  brightBlue: "#839496",
  brightMagenta: "#6c71c4",
  brightCyan: "#93a1a1",
  brightWhite: "#fdf6e3",
};

// === SECTION 4 END ===

// Gruvbox Dark — retro groove
const gruvboxDark: ITheme = {
  background: "#282828",
  foreground: "#ebdbb2",
  cursor: "#ebdbb2",
  selectionBackground: "#504945",
  black: "#282828",
  red: "#cc241d",
  green: "#98971a",
  yellow: "#d79921",
  blue: "#458588",
  magenta: "#b16286",
  cyan: "#689d6a",
  white: "#a89984",
  brightBlack: "#928374",
  brightRed: "#fb4934",
  brightGreen: "#b8bb26",
  brightYellow: "#fabd2f",
  brightBlue: "#83a598",
  brightMagenta: "#d3869b",
  brightCyan: "#8ec07c",
  brightWhite: "#ebdbb2",
};

// Nord — arctic, north-bluish
const nord: ITheme = {
  background: "#2e3440",
  foreground: "#d8dee9",
  cursor: "#d8dee9",
  selectionBackground: "#434c5e",
  black: "#3b4252",
  red: "#bf616a",
  green: "#a3be8c",
  yellow: "#ebcb8b",
  blue: "#81a1c1",
  magenta: "#b48ead",
  cyan: "#88c0d0",
  white: "#e5e9f0",
  brightBlack: "#4c566a",
  brightRed: "#bf616a",
  brightGreen: "#a3be8c",
  brightYellow: "#ebcb8b",
  brightBlue: "#81a1c1",
  brightMagenta: "#b48ead",
  brightCyan: "#8fbcbb",
  brightWhite: "#eceff4",
};

// === SECTION 5 END ===

// Tokyo Night
const tokyoNight: ITheme = {
  background: "#1a1b26",
  foreground: "#a9b1d6",
  cursor: "#c0caf5",
  selectionBackground: "#33467c",
  black: "#15161e",
  red: "#f7768e",
  green: "#9ece6a",
  yellow: "#e0af68",
  blue: "#7aa2f7",
  magenta: "#bb9af7",
  cyan: "#7dcfff",
  white: "#a9b1d6",
  brightBlack: "#414868",
  brightRed: "#f7768e",
  brightGreen: "#9ece6a",
  brightYellow: "#e0af68",
  brightBlue: "#7aa2f7",
  brightMagenta: "#bb9af7",
  brightCyan: "#7dcfff",
  brightWhite: "#c0caf5",
};

// GitHub Dark
const githubDark: ITheme = {
  background: "#0d1117",
  foreground: "#c9d1d9",
  cursor: "#73b7f0",
  selectionBackground: "#264f78",
  black: "#484f58",
  red: "#ff7b72",
  green: "#3fb950",
  yellow: "#d29922",
  blue: "#58a6ff",
  magenta: "#bc8cff",
  cyan: "#39c5cf",
  white: "#b1bac4",
  brightBlack: "#6e7681",
  brightRed: "#ffa198",
  brightGreen: "#56d364",
  brightYellow: "#e3b341",
  brightBlue: "#79c0ff",
  brightMagenta: "#d2a8ff",
  brightCyan: "#56d4dd",
  brightWhite: "#f0f6fc",
};

// === SECTION 6 END ===

export const TERMINAL_THEMES: TerminalThemePreset[] = [
  { id: "catppuccin-mocha", name: "Catppuccin Mocha", isDark: true, theme: catppuccinMocha },
  { id: "catppuccin-latte", name: "Catppuccin Latte", isDark: false, theme: catppuccinLatte },
  { id: "dracula", name: "Dracula", isDark: true, theme: dracula },
  { id: "one-dark", name: "One Dark", isDark: true, theme: oneDark },
  { id: "solarized-dark", name: "Solarized Dark", isDark: true, theme: solarizedDark },
  { id: "solarized-light", name: "Solarized Light", isDark: false, theme: solarizedLight },
  { id: "gruvbox-dark", name: "Gruvbox Dark", isDark: true, theme: gruvboxDark },
  { id: "nord", name: "Nord", isDark: true, theme: nord },
  { id: "tokyo-night", name: "Tokyo Night", isDark: true, theme: tokyoNight },
  { id: "github-dark", name: "GitHub Dark", isDark: true, theme: githubDark },
];

const THEME_MAP = new Map(TERMINAL_THEMES.map((t) => [t.id, t]));

/**
 * Get a terminal theme preset by ID.
 * Falls back to Catppuccin Mocha if the ID is unknown.
 */
export function getTerminalTheme(themeId: string): TerminalThemePreset {
  return THEME_MAP.get(themeId) ?? TERMINAL_THEMES[0];
}
