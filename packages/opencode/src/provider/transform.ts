import type { ModelMessage } from "ai"
import { mergeDeep, unique } from "remeda"
import type { JSONSchema7 } from "@ai-sdk/provider"
import type { JSONSchema } from "zod/v4/core"
import type { Provider } from "./provider"
import type { ModelsDev } from "./models"
import { Flag } from "@/flag/flag"

type Modality = NonNullable<ModelsDev.Model["modalities"]>["input"][number]

function mimeToModality(mime: string): Modality | undefined {
  if (mime.startsWith("image/")) return "image"
  if (mime.startsWith("audio/")) return "audio"
  if (mime.startsWith("video/")) return "video"
  if (mime === "application/pdf") return "pdf"
  return undefined
}

export namespace ProviderTransform {
  export const OUTPUT_TOKEN_MAX = Flag.OPENCODE_EXPERIMENTAL_OUTPUT_TOKEN_MAX || 32_000

  function sdkKey(npm: string): string | undefined {
    switch (npm) {
      case "@ai-sdk/openai-compatible":
        return "openaiCompatible"
    }
    return undefined
  }

  function normalizeMessages(
    msgs: ModelMessage[],
    model: Provider.Model,
    options: Record<string, unknown>,
  ): ModelMessage[] {
    if (model.api.id.includes("claude")) {
      const scrub = (id: string) => id.replace(/[^a-zA-Z0-9_-]/g, "_")
      return msgs.map((msg) => {
        if (msg.role === "assistant" && Array.isArray(msg.content)) {
          return {
            ...msg,
            content: msg.content.map((part) => {
              if (part.type === "tool-call" || part.type === "tool-result") {
                return { ...part, toolCallId: scrub(part.toolCallId) }
              }
              return part
            }),
          }
        }
        if (msg.role === "tool" && Array.isArray(msg.content)) {
          return {
            ...msg,
            content: msg.content.map((part) => {
              if (part.type === "tool-result") {
                return { ...part, toolCallId: scrub(part.toolCallId) }
              }
              return part
            }),
          }
        }
        return msg
      })
    }

    if (typeof model.capabilities.interleaved === "object" && model.capabilities.interleaved.field) {
      const field = model.capabilities.interleaved.field
      return msgs.map((msg) => {
        if (msg.role === "assistant" && Array.isArray(msg.content)) {
          const reasoningParts = msg.content.filter((part: any) => part.type === "reasoning")
          const reasoningText = reasoningParts.map((part: any) => part.text).join("")

          const filteredContent = msg.content.filter((part: any) => part.type !== "reasoning")

          if (reasoningText) {
            return {
              ...msg,
              content: filteredContent,
              providerOptions: {
                ...msg.providerOptions,
                openaiCompatible: {
                  ...(msg.providerOptions as any)?.openaiCompatible,
                  [field]: reasoningText,
                },
              },
            }
          }

          return {
            ...msg,
            content: filteredContent,
          }
        }

        return msg
      })
    }

    return msgs
  }

  function applyCaching(msgs: ModelMessage[], model: Provider.Model): ModelMessage[] {
    const system = msgs.filter((msg) => msg.role === "system").slice(0, 2)
    const final = msgs.filter((msg) => msg.role !== "system").slice(-2)

    const providerOptions = {
      openaiCompatible: {
        cache_control: { type: "ephemeral" },
      },
    }

    for (const msg of unique([...system, ...final])) {
      const shouldUseContentOptions = Array.isArray(msg.content) && msg.content.length > 0

      if (shouldUseContentOptions) {
        const lastContent = msg.content[msg.content.length - 1]
        if (
          lastContent &&
          typeof lastContent === "object" &&
          lastContent.type !== "tool-approval-request" &&
          lastContent.type !== "tool-approval-response"
        ) {
          lastContent.providerOptions = mergeDeep(lastContent.providerOptions ?? {}, providerOptions)
          continue
        }
      }

      msg.providerOptions = mergeDeep(msg.providerOptions ?? {}, providerOptions)
    }

    return msgs
  }

  function unsupportedParts(msgs: ModelMessage[], model: Provider.Model): ModelMessage[] {
    return msgs.map((msg) => {
      if (msg.role !== "user" || !Array.isArray(msg.content)) return msg

      const filtered = msg.content.map((part) => {
        if (part.type !== "file" && part.type !== "image") return part

        // Check for empty base64 image data
        if (part.type === "image") {
          const imageStr = part.image.toString()
          if (imageStr.startsWith("data:")) {
            const match = imageStr.match(/^data:([^;]+);base64,(.*)$/)
            if (match && (!match[2] || match[2].length === 0)) {
              return {
                type: "text" as const,
                text: "ERROR: Image file is empty or corrupted. Please provide a valid image.",
              }
            }
          }
        }

        const mime = part.type === "image" ? part.image.toString().split(";")[0].replace("data:", "") : part.mediaType
        const filename = part.type === "file" ? part.filename : undefined
        const modality = mimeToModality(mime)
        if (!modality) return part
        if (model.capabilities.input[modality]) return part

        const name = filename ? `"${filename}"` : modality
        return {
          type: "text" as const,
          text: `ERROR: Cannot read ${name} (this model does not support ${modality} input). Inform the user.`,
        }
      })

      return { ...msg, content: filtered }
    })
  }

  export function message(msgs: ModelMessage[], model: Provider.Model, options: Record<string, unknown>) {
    msgs = unsupportedParts(msgs, model)
    msgs = normalizeMessages(msgs, model, options)
    if (model.api.id.includes("claude") || model.id.includes("claude")) {
      msgs = applyCaching(msgs, model)
    }

    // Remap providerOptions keys from stored providerID to expected SDK key
    const key = sdkKey(model.api.npm)
    if (key && key !== model.providerID) {
      const remap = (opts: Record<string, any> | undefined) => {
        if (!opts) return opts
        if (!(model.providerID in opts)) return opts
        const result = { ...opts }
        result[key] = result[model.providerID]
        delete result[model.providerID]
        return result
      }

      msgs = msgs.map((msg) => {
        if (!Array.isArray(msg.content)) return { ...msg, providerOptions: remap(msg.providerOptions) }
        return {
          ...msg,
          providerOptions: remap(msg.providerOptions),
          content: msg.content.map((part) => {
            if (part.type === "tool-approval-request" || part.type === "tool-approval-response") {
              return { ...part }
            }
            return { ...part, providerOptions: remap(part.providerOptions) }
          }),
        } as typeof msg
      })
    }

    return msgs
  }

  export function temperature(model: Provider.Model) {
    const id = model.id.toLowerCase()
    if (id.includes("qwen")) return 0.55
    if (id.includes("claude")) return undefined
    if (id.includes("gemini")) return 1.0
    if (id.includes("glm-4.6")) return 1.0
    if (id.includes("glm-4.7")) return 1.0
    if (id.includes("minimax-m2")) return 1.0
    if (id.includes("kimi-k2")) {
      // kimi-k2-thinking & kimi-k2.5 && kimi-k2p5 && kimi-k2-5
      if (["thinking", "k2.", "k2p", "k2-5"].some((s) => id.includes(s))) {
        return 1.0
      }
      return 0.6
    }
    return undefined
  }

  export function topP(model: Provider.Model) {
    const id = model.id.toLowerCase()
    if (id.includes("qwen")) return 1
    if (["minimax-m2", "gemini", "kimi-k2.5", "kimi-k2p5", "kimi-k2-5"].some((s) => id.includes(s))) {
      return 0.95
    }
    return undefined
  }

  export function topK(model: Provider.Model) {
    const id = model.id.toLowerCase()
    if (id.includes("minimax-m2")) {
      if (["m2.", "m25", "m21"].some((s) => id.includes(s))) return 40
      return 20
    }
    if (id.includes("gemini")) return 64
    return undefined
  }

  const WIDELY_SUPPORTED_EFFORTS = ["low", "medium", "high"]

  export function variants(model: Provider.Model): Record<string, Record<string, any>> {
    if (!model.capabilities.reasoning) return {}

    const id = model.id.toLowerCase()
    if (
      id.includes("deepseek") ||
      id.includes("minimax") ||
      id.includes("glm") ||
      id.includes("mistral") ||
      id.includes("kimi") ||
      id.includes("k2p5")
    )
      return {}

    if (id.includes("grok")) return {}

    if (model.api.npm === "@ai-sdk/openai-compatible") {
      return Object.fromEntries(WIDELY_SUPPORTED_EFFORTS.map((effort) => [effort, { reasoningEffort: effort }]))
    }

    return {}
  }

  export function options(input: {
    model: Provider.Model
    sessionID: string
    providerOptions?: Record<string, any>
  }): Record<string, any> {
    const result: Record<string, any> = {}

    if (
      input.model.providerID === "baseten" ||
      (input.model.providerID === "opencode" && ["kimi-k2-thinking", "glm-4.6"].includes(input.model.api.id))
    ) {
      result["chat_template_args"] = { enable_thinking: true }
    }

    if (["zai", "zhipuai"].includes(input.model.providerID) && input.model.api.npm === "@ai-sdk/openai-compatible") {
      result["thinking"] = {
        type: "enabled",
        clear_thinking: false,
      }
    }

    if (input.model.providerID === "openai" || input.providerOptions?.setCacheKey) {
      result["promptCacheKey"] = input.sessionID
    }

    if (input.model.providerID === "opencode" && input.model.capabilities.reasoning) {
      result["enable_thinking"] = true
    }

    if (
      input.model.providerID === "alibaba-cn" &&
      input.model.capabilities.reasoning &&
      input.model.api.npm === "@ai-sdk/openai-compatible"
    ) {
      result["enable_thinking"] = true
    }

    return result
  }

  export function smallOptions(model: Provider.Model) {
    if (model.providerID === "openai" && model.api.id.includes("gpt-5")) {
      if (model.api.id.includes("5.")) {
        return { store: false, reasoningEffort: "low" }
      }
      return { store: false, reasoningEffort: "minimal" }
    }
    return {}
  }

  export function providerOptions(model: Provider.Model, options: { [x: string]: any }) {
    const key = sdkKey(model.api.npm) ?? model.providerID
    return { [key]: options }
  }

  export function maxOutputTokens(model: Provider.Model): number {
    return Math.min(model.limit.output, OUTPUT_TOKEN_MAX) || OUTPUT_TOKEN_MAX
  }

  export function schema(model: Provider.Model, schema: JSONSchema.BaseSchema | JSONSchema7): JSONSchema7 {
    return schema as JSONSchema7
  }
}
