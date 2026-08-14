/**
 * Theme tokens, adopted from opencode.
 *
 * These JSON files are opencode's design system as data: palettes and syntax
 * overrides with no dependency on its HTTP protocol or Effect runtime. Adopting
 * them is the part of "inherit opencode's UX" that survives the decision not to
 * adopt its backend, and it is why the app looks like a sibling of opencode
 * rather than a generic webview.
 */

import opencode from "./themes/opencode.json"
import tokyonight from "./themes/tokyonight.json"
import catppuccin from "./themes/catppuccin.json"
import gruvbox from "./themes/gruvbox.json"
import nord from "./themes/nord.json"
import dracula from "./themes/dracula.json"
import everforest from "./themes/everforest.json"
import rosepine from "./themes/rosepine.json"
import vesper from "./themes/vesper.json"
import matrix from "./themes/matrix.json"

export type Palette = {
  neutral: string
  ink: string
  primary: string
  accent: string
  success: string
  warning: string
  error: string
  info: string
  diffAdd: string
  diffDelete: string
}

export type ThemeMode = { palette: Palette; overrides?: Record<string, string> }
export type Theme = { id: string; name: string; light: ThemeMode; dark: ThemeMode }

export const THEMES: Theme[] = [
  opencode,
  tokyonight,
  catppuccin,
  gruvbox,
  nord,
  dracula,
  everforest,
  rosepine,
  vesper,
  matrix,
] as unknown as Theme[]

export const DEFAULT_THEME_ID = "opencode"

/**
 * Blend two hex colours.
 *
 * Surfaces are derived from the palette rather than hardcoded so that every
 * adopted theme produces a coherent set of panel backgrounds, instead of only
 * the two the palette happens to name.
 */
export function mix(from: string, to: string, amount: number): string {
  const parse = (hex: string) => {
    const clean = hex.replace("#", "")
    const full =
      clean.length === 3
        ? clean
            .split("")
            .map((c) => c + c)
            .join("")
        : clean
    return [
      parseInt(full.slice(0, 2), 16),
      parseInt(full.slice(2, 4), 16),
      parseInt(full.slice(4, 6), 16),
    ]
  }
  const a = parse(from)
  const b = parse(to)
  const blended = a.map((channel, index) =>
    Math.round(channel + (b[index] - channel) * amount),
  )
  return "#" + blended.map((c) => c.toString(16).padStart(2, "0")).join("")
}

/** Push a theme's tokens onto the document as CSS custom properties. */
export function applyTheme(theme: Theme, dark: boolean): void {
  const mode = dark ? theme.dark : theme.light
  const p = mode.palette
  const root = document.documentElement
  const set = (name: string, value: string) => root.style.setProperty(name, value)

  set("--bg", p.neutral)
  set("--bg-panel", mix(p.neutral, p.ink, 0.04))
  set("--bg-raised", mix(p.neutral, p.ink, 0.08))
  set("--border", mix(p.neutral, p.ink, 0.16))
  set("--text", p.ink)
  set("--text-weak", mode.overrides?.["text-weak"] ?? mix(p.ink, p.neutral, 0.42))
  set("--primary", p.primary)
  set("--accent", p.accent)
  set("--success", p.success)
  set("--warning", p.warning)
  set("--error", p.error)
  set("--info", p.info)
  root.style.colorScheme = dark ? "dark" : "light"
}
