import z from "zod"
import { Env } from "../env"
import { Log } from "../util/log"
import type { ModelsDev } from "./models"

export namespace Ollama {
  const log = Log.create({ service: "ollama" })

  export const ID = "ollama"
  export const NAME = "Ollama"
  export const DEFAULT_HOST = "http://127.0.0.1:11434"

  /** Ollama reports the window under `<architecture>.context_length`. */
  const CONTEXT_SUFFIX = ".context_length"
  const FALLBACK_CONTEXT = 8192
  const FALLBACK_OUTPUT = 4096

  export const Tag = z.object({
    name: z.string(),
    model: z.string().optional(),
    size: z.number().optional(),
    details: z
      .object({
        family: z.string().optional(),
        families: z.array(z.string()).nullish(),
        parameter_size: z.string().optional(),
        quantization_level: z.string().optional(),
      })
      .optional(),
  })
  export type Tag = z.infer<typeof Tag>

  export const Show = z.object({
    capabilities: z.array(z.string()).nullish(),
    model_info: z.record(z.string(), z.any()).nullish(),
    details: z
      .object({
        family: z.string().optional(),
        families: z.array(z.string()).nullish(),
      })
      .optional(),
  })
  export type Show = z.infer<typeof Show>

  /**
   * OLLAMA_HOST is commonly set bare (`127.0.0.1:11434`) or with a trailing
   * slash, neither of which is a usable base URL.
   */
  export function normalize(host?: string) {
    const raw = host?.trim()
    if (!raw) return DEFAULT_HOST
    const full = /^https?:\/\//.test(raw) ? raw : `http://${raw}`
    return full.replace(/\/+$/, "")
  }

  export function host() {
    return normalize(Env.get("OLLAMA_HOST"))
  }

  export function context(info?: Record<string, unknown> | null) {
    if (!info) return FALLBACK_CONTEXT
    const arch = info["general.architecture"]
    const direct = typeof arch === "string" ? info[`${arch}${CONTEXT_SUFFIX}`] : undefined
    if (typeof direct === "number" && direct > 0) return direct
    // Architecture key can disagree with the prefix actually used, so fall back to any match.
    const any = Object.entries(info).find(([key, value]) => key.endsWith(CONTEXT_SUFFIX) && typeof value === "number")
    if (any && (any[1] as number) > 0) return any[1] as number
    return FALLBACK_CONTEXT
  }

  /** Output is not advertised, so reserve a slice of the window without exceeding it. */
  export function output(ctx: number) {
    return Math.max(1024, Math.min(FALLBACK_OUTPUT, Math.floor(ctx / 2)))
  }

  export function toModel(tag: Tag, show?: Show): ModelsDev.Model {
    const caps = show?.capabilities ?? []
    const ctx = context(show?.model_info)
    const vision = caps.includes("vision")
    return {
      id: tag.name,
      name: tag.name,
      family: show?.details?.family ?? tag.details?.family,
      release_date: "",
      attachment: vision,
      reasoning: caps.includes("thinking"),
      temperature: true,
      tool_call: caps.includes("tools"),
      // Local inference is free; keeping zeros makes cost UI collapse to nothing.
      cost: { input: 0, output: 0, cache_read: 0, cache_write: 0 },
      limit: { context: ctx, output: output(ctx) },
      modalities: {
        input: vision ? ["text", "image"] : ["text"],
        output: ["text"],
      },
      options: {},
    }
  }

  async function fetchJson(url: string, init?: RequestInit) {
    const res = await fetch(url, init).catch((err) => {
      log.info("request failed", { url, err: String(err) })
      return undefined
    })
    if (!res?.ok) return undefined
    return res.json().catch(() => undefined)
  }

  export async function tags(base = host()) {
    const body = await fetchJson(`${base}/api/tags`)
    if (!body) return []
    const parsed = z.object({ models: z.array(Tag).nullish() }).safeParse(body)
    if (!parsed.success) return []
    return parsed.data.models ?? []
  }

  export async function show(name: string, base = host()) {
    const body = await fetchJson(`${base}/api/show`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ model: name }),
    })
    if (!body) return undefined
    const parsed = Show.safeParse(body)
    if (!parsed.success) return undefined
    return parsed.data
  }

  /** True when a daemon is reachable, regardless of whether any model is pulled. */
  export async function running(base = host()) {
    const res = await fetch(`${base}/api/tags`).catch(() => undefined)
    return res?.ok === true
  }

  /**
   * Detects every locally pulled model. Returns a provider with no models when
   * the daemon is unreachable so onboarding can tell "no Ollama" from "no models".
   */
  export async function list(base = host()): Promise<ModelsDev.Provider> {
    const found = await tags(base)
    const models = await Promise.all(
      found.map(async (tag) => [tag.name, toModel(tag, await show(tag.name, base))] as const),
    )
    log.info("detected models", { count: models.length, base })
    return {
      id: ID,
      name: NAME,
      api: `${base}/v1`,
      npm: "@ai-sdk/openai-compatible",
      env: [],
      models: Object.fromEntries(models),
    }
  }
}
