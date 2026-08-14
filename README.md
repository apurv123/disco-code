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
  design system and the shape of the interface layer.
- **[oh-my-codex](https://github.com/Yeachan-Heo/oh-my-codex)** contributes the
  design of the staged prompt-enhancement harness.

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

On top of that, a request can be run as a staged harness rather than a single
turn: clarify what is ambiguous, plan before editing, harden anything
irreversible, execute, then verify the result against the criteria the plan
stated. Which stages run is decided per request, so `fix the typo in README.md`
stays one turn while `clean this up` earns the full pipeline. Run
`claw enhance "<request>"` to see the decision without spending any inference,
or pass `--enhance` to a normal run to use it.

## Requirements

- [Ollama](https://ollama.com), running, with at least one model pulled
- Rust 1.90 or newer (to build from source)
- Node.js 20 or newer (to build the desktop interface)

## Building

The CLI and core:

```sh
cargo build --workspace
cargo test --workspace
```

The desktop app:

```sh
cd desktop-ui && npm install && cd ..
cargo build -p desktop            # run it from target/
cd crates/desktop && cargo tauri dev    # or develop with hot reload
```

To produce installers (`.msi` and `.exe` on Windows, `.dmg` on macOS,
`.deb`/`.AppImage` on Linux):

```sh
cd crates/desktop && cargo tauri build
```

## Roadmap

| Phase | Scope | Status |
| ----- | ----- | ------ |
| A | Foundation: Rust core, licensing, attribution | done |
| B | Ollama-only inference with live model detection | done |
| B2 | Removal of all hosted-provider code paths | done |
| C | Egress policy enforcement | done |
| D | Prompt-enhancement pipeline tuned for local models | done |
| E | Tauri desktop shell and interface layer | done |
| F | Tooling: MCP, web search, code search | planned |

## License

MIT. Derived from claw-code and opencode, both MIT. See [LICENSE](LICENSE) and
[NOTICE](NOTICE).
