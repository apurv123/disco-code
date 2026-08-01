import { test, expect, describe } from "bun:test"
import { Ollama } from "../../src/provider/ollama"

describe("Ollama.normalize", () => {
  test("defaults when unset or blank", () => {
    expect(Ollama.normalize()).toBe("http://127.0.0.1:11434")
    expect(Ollama.normalize("")).toBe("http://127.0.0.1:11434")
    expect(Ollama.normalize("   ")).toBe("http://127.0.0.1:11434")
  })

  test("adds scheme to a bare host:port", () => {
    expect(Ollama.normalize("127.0.0.1:11434")).toBe("http://127.0.0.1:11434")
    expect(Ollama.normalize("box.local:1234")).toBe("http://box.local:1234")
  })

  test("keeps an explicit scheme and strips trailing slashes", () => {
    expect(Ollama.normalize("https://box.local:1234")).toBe("https://box.local:1234")
    expect(Ollama.normalize("http://127.0.0.1:11434/")).toBe("http://127.0.0.1:11434")
    expect(Ollama.normalize("http://127.0.0.1:11434///")).toBe("http://127.0.0.1:11434")
  })
})

describe("Ollama.context", () => {
  test("reads the window for the declared architecture", () => {
    expect(Ollama.context({ "general.architecture": "qwen2", "qwen2.context_length": 32768 })).toBe(32768)
  })

  test("falls back to any context_length when the architecture key disagrees", () => {
    expect(Ollama.context({ "general.architecture": "nope", "llama.context_length": 131072 })).toBe(131072)
  })

  test("falls back when info is missing, empty or non-numeric", () => {
    expect(Ollama.context()).toBe(8192)
    expect(Ollama.context(null)).toBe(8192)
    expect(Ollama.context({})).toBe(8192)
    expect(Ollama.context({ "llama.context_length": "131072" })).toBe(8192)
    expect(Ollama.context({ "llama.context_length": 0 })).toBe(8192)
  })
})

describe("Ollama.output", () => {
  test("never exceeds half the context window", () => {
    expect(Ollama.output(4096)).toBe(2048)
    expect(Ollama.output(2048)).toBe(1024)
  })

  test("caps at the default and keeps a usable floor", () => {
    expect(Ollama.output(131072)).toBe(4096)
    expect(Ollama.output(512)).toBe(1024)
  })
})

describe("Ollama.toModel", () => {
  const tag = { name: "qwen2.5-coder:7b", details: { family: "qwen2" } }

  test("maps capabilities reported by the daemon", () => {
    const model = Ollama.toModel(tag, {
      capabilities: ["completion", "tools", "vision", "thinking"],
      model_info: { "general.architecture": "qwen2", "qwen2.context_length": 32768 },
    })
    expect(model.tool_call).toBe(true)
    expect(model.reasoning).toBe(true)
    expect(model.attachment).toBe(true)
    expect(model.modalities?.input).toEqual(["text", "image"])
    expect(model.limit.context).toBe(32768)
  })

  test("treats absent capabilities as unsupported", () => {
    const model = Ollama.toModel(tag, { capabilities: ["completion"] })
    expect(model.tool_call).toBe(false)
    expect(model.reasoning).toBe(false)
    expect(model.attachment).toBe(false)
    expect(model.modalities?.input).toEqual(["text"])
  })

  test("is safe when /api/show is unavailable", () => {
    const model = Ollama.toModel(tag)
    expect(model.id).toBe("qwen2.5-coder:7b")
    expect(model.family).toBe("qwen2")
    expect(model.tool_call).toBe(false)
    expect(model.limit.context).toBe(8192)
  })

  test("reports local inference as free", () => {
    const model = Ollama.toModel(tag)
    expect(model.cost).toEqual({ input: 0, output: 0, cache_read: 0, cache_write: 0 })
  })
})

/** Serves the subset of the Ollama HTTP API that detection relies on. */
function fake(models: Record<string, { capabilities?: string[]; context?: number }>) {
  return Bun.serve({
    port: 0,
    async fetch(req) {
      const url = new URL(req.url)
      if (url.pathname === "/api/tags") {
        return Response.json({
          models: Object.keys(models).map((name) => ({ name, model: name, details: { family: "test" } })),
        })
      }
      if (url.pathname === "/api/show") {
        const body = (await req.json()) as { model: string }
        const entry = models[body.model]
        if (!entry) return new Response("not found", { status: 404 })
        return Response.json({
          capabilities: entry.capabilities ?? ["completion"],
          model_info: { "general.architecture": "test", "test.context_length": entry.context ?? 8192 },
        })
      }
      return new Response("not found", { status: 404 })
    },
  })
}

describe("Ollama.list", () => {
  test("detects every pulled model with its capabilities", async () => {
    const server = fake({
      "qwen2.5-coder:7b": { capabilities: ["completion", "tools"], context: 32768 },
      "llava:13b": { capabilities: ["completion", "vision"], context: 4096 },
    })
    const provider = await Ollama.list(server.url.origin)
    await server.stop(true)

    expect(provider.id).toBe("ollama")
    expect(Object.keys(provider.models).sort()).toEqual(["llava:13b", "qwen2.5-coder:7b"])
    expect(provider.models["qwen2.5-coder:7b"].tool_call).toBe(true)
    expect(provider.models["qwen2.5-coder:7b"].limit.context).toBe(32768)
    expect(provider.models["llava:13b"].attachment).toBe(true)
    expect(provider.models["llava:13b"].tool_call).toBe(false)
  })

  test("points at the OpenAI-compatible endpoint the daemon exposes", async () => {
    const server = fake({ "a:1": {} })
    const provider = await Ollama.list(server.url.origin)
    await server.stop(true)
    expect(provider.api).toBe(`${server.url.origin}/v1`)
  })

  test("returns an empty provider when no models are pulled", async () => {
    const server = fake({})
    const provider = await Ollama.list(server.url.origin)
    await server.stop(true)
    expect(provider.models).toEqual({})
  })

  test("degrades to an empty provider when the daemon is unreachable", async () => {
    const server = fake({})
    const origin = server.url.origin
    await server.stop(true)
    const provider = await Ollama.list(origin)
    expect(provider.id).toBe("ollama")
    expect(provider.models).toEqual({})
  })

  test("still lists a model when /api/show fails for it", async () => {
    const server = Bun.serve({
      port: 0,
      fetch(req) {
        if (new URL(req.url).pathname === "/api/tags") {
          return Response.json({ models: [{ name: "broken:1" }] })
        }
        return new Response("boom", { status: 500 })
      },
    })
    const provider = await Ollama.list(server.url.origin)
    await server.stop(true)
    expect(Object.keys(provider.models)).toEqual(["broken:1"])
    expect(provider.models["broken:1"].limit.context).toBe(8192)
  })
})

describe("Ollama.running", () => {
  test("is true for a reachable daemon and false otherwise", async () => {
    const server = fake({})
    const origin = server.url.origin
    expect(await Ollama.running(origin)).toBe(true)
    await server.stop(true)
    expect(await Ollama.running(origin)).toBe(false)
  })
})
