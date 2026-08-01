import z from "zod"
import { Flag } from "../flag/flag"
import { lazy } from "@/util/lazy"
import { Filesystem } from "../util/filesystem"
import { Ollama } from "./ollama"

export namespace ModelsDev {
  export const Model = z.object({
    id: z.string(),
    name: z.string(),
    family: z.string().optional(),
    release_date: z.string(),
    attachment: z.boolean(),
    reasoning: z.boolean(),
    temperature: z.boolean(),
    tool_call: z.boolean(),
    interleaved: z
      .union([
        z.literal(true),
        z
          .object({
            field: z.enum(["reasoning_content", "reasoning_details"]),
          })
          .strict(),
      ])
      .optional(),
    cost: z
      .object({
        input: z.number(),
        output: z.number(),
        cache_read: z.number().optional(),
        cache_write: z.number().optional(),
        context_over_200k: z
          .object({
            input: z.number(),
            output: z.number(),
            cache_read: z.number().optional(),
            cache_write: z.number().optional(),
          })
          .optional(),
      })
      .optional(),
    limit: z.object({
      context: z.number(),
      input: z.number().optional(),
      output: z.number(),
    }),
    modalities: z
      .object({
        input: z.array(z.enum(["text", "audio", "image", "video", "pdf"])),
        output: z.array(z.enum(["text", "audio", "image", "video", "pdf"])),
      })
      .optional(),
    experimental: z.boolean().optional(),
    status: z.enum(["alpha", "beta", "deprecated"]).optional(),
    options: z.record(z.string(), z.any()),
    headers: z.record(z.string(), z.string()).optional(),
    provider: z.object({ npm: z.string().optional(), api: z.string().optional() }).optional(),
    variants: z.record(z.string(), z.record(z.string(), z.any())).optional(),
  })
  export type Model = z.infer<typeof Model>

  export const Provider = z.object({
    api: z.string().optional(),
    name: z.string(),
    env: z.array(z.string()),
    id: z.string(),
    npm: z.string().optional(),
    models: z.record(z.string(), Model),
  })

  export type Provider = z.infer<typeof Provider>

  /**
   * Local model registry. Unlike upstream there is no remote catalog to sync:
   * the only provider is Ollama, discovered live from the running daemon.
   * OPENCODE_MODELS_PATH still overrides for tests and offline fixtures.
   */
  export const Data = lazy(async () => {
    const override = Flag.OPENCODE_MODELS_PATH
    if (override) {
      const result = await Filesystem.readJson(override).catch(() => undefined)
      if (result) return result as Record<string, Provider>
    }
    const ollama = await Ollama.list()
    return { [Ollama.ID]: ollama } as Record<string, Provider>
  })

  export async function get() {
    return await Data()
  }

  /** Re-detects models; call after the user pulls or removes one. */
  export async function refresh(_force?: boolean) {
    ModelsDev.Data.reset()
  }
}
