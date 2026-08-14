import { describe, expect, test } from "vitest"
import { applyTheme, mix, THEMES, DEFAULT_THEME_ID, type Theme } from "./theme"

describe("theme tokens adopted from opencode", () => {
  test("every bundled theme carries both modes and a full palette", () => {
    expect(THEMES.length).toBeGreaterThan(0)
    for (const theme of THEMES) {
      expect(theme.id, "a theme without an id cannot be selected").toBeTruthy()
      expect(theme.name).toBeTruthy()
      for (const mode of [theme.light, theme.dark]) {
        // A missing palette entry surfaces as an unreadable interface rather
        // than an error, so it is checked rather than assumed.
        for (const key of ["neutral", "ink", "primary", "error"] as const) {
          expect(mode.palette[key], `${theme.id} is missing ${key}`).toMatch(
            /^#[0-9a-fA-F]{3,8}$/,
          )
        }
      }
    }
  })

  test("the default theme is actually bundled", () => {
    expect(THEMES.find((t) => t.id === DEFAULT_THEME_ID)).toBeDefined()
  })
})

describe("mix", () => {
  test("returns the endpoints unchanged", () => {
    expect(mix("#000000", "#ffffff", 0)).toBe("#000000")
    expect(mix("#000000", "#ffffff", 1)).toBe("#ffffff")
  })

  test("blends toward the target", () => {
    expect(mix("#000000", "#ffffff", 0.5)).toBe("#808080")
  })

  test("expands three-digit hex", () => {
    // Themes are hand-authored JSON, so shorthand hex is a realistic input and
    // silently producing NaN channels would tint the whole interface.
    expect(mix("#fff", "#000", 0)).toBe("#ffffff")
  })
})

describe("applyTheme", () => {
  test("derives surface tokens rather than leaving them unset", () => {
    const theme = THEMES.find((t) => t.id === DEFAULT_THEME_ID) as Theme
    applyTheme(theme, true)

    const style = document.documentElement.style
    for (const token of ["--bg", "--bg-panel", "--border", "--text", "--primary"]) {
      expect(style.getPropertyValue(token), `${token} was not set`).not.toBe("")
    }
    expect(style.colorScheme).toBe("dark")
  })
})
