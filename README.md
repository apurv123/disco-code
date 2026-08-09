# Disco Code

A local-inference coding agent, packaged as an installable Rust desktop
application. Disco Code runs entirely against models you have already pulled
with [Ollama](https://ollama.com) — there is no hosted model provider, no API
key, and no account.

> **Status:** early. The Rust core is in place and the desktop shell is being
> assembled. See [Roadmap](#roadmap).

## What it is

Disco Code combines two MIT-licensed open source projects into one product:

- **[claw-code](https://github.com/ultraworkers/claw-code)** contributes the
  Rust core — session management, permissions and policy enforcement, sandboxing,
  MCP lifecycle, the tool suite, and the prompt-construction pipeline.
- **[opencode](https://github.com/anomalyco/opencode)** contributes the
  interface layer and developer experience.

Disco Code is itself MIT licensed. See [LICENSE](LICENSE) for the combined
notice and [NOTICE](NOTICE) for a per-component attribution map.

## Principles

**Local inference, hardcoded.** Ollama is the only inference backend. Models are
detected live from your running daemon, including their real context windows and
capabilities — nothing is read from a baked-in catalog, so a model you pulled
five minutes ago is simply available.

**Local is not offline.** Disco Code is expected to reach the network. Web
search, web fetch, code search, and Model Context Protocol servers such as
Playwright all work normally. The single thing it will not do is send your code
to a cloud model: hosted AI providers are removed from the tree, and an egress
policy blocks their endpoints at runtime so the guarantee survives refactoring.

**Small models, good answers.** Local models are weaker than frontier ones, so
the scaffolding around the prompt matters more, not less. Disco Code inherits
claw-code's prompt-construction pipeline — project context discovery,
instruction-file resolution, git state, and budgeted context assembly — and
tunes it for local inference.

## Requirements

- [Ollama](https://ollama.com), running, with at least one model pulled
- Rust 1.90 or newer (to build from source)

## Building

```sh
cargo build --workspace
cargo test --workspace
```

## Roadmap

| Phase | Scope | Status |
| ----- | ----- | ------ |
| A | Foundation: Rust core, licensing, attribution | done |
| B | Ollama-only inference with live model detection | done |
| B2 | Removal of all hosted-provider code paths | done |
| C | Egress policy enforcement | planned |
| D | Prompt-enhancement pipeline tuned for local models | planned |
| E | Tauri desktop shell and interface layer | planned |
| F | Tooling: MCP, web search, code search | planned |

## License

MIT. Derived from claw-code and opencode, both MIT. See [LICENSE](LICENSE) and
[NOTICE](NOTICE).
