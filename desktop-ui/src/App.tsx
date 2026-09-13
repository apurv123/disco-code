import { createEffect, createResource, createSignal, For, Show, onMount } from "solid-js"
import {
  cancelTurn,
  chooseProjectFolder,
  daemonStatus,
  sendPrompt,
  triageRequest,
  type Model,
  type Triage,
  type TurnEvent,
} from "./bridge"
import {
  emptyChat,
  load as loadChats,
  save as saveChats,
  titleFrom,
  type Chat,
  type Entry,
} from "./chats"
import { applyTheme, DEFAULT_THEME_ID, THEMES } from "./theme"

/** Live progress for one chat's turn. Absent when that chat is idle. */
type Run = {
  stage: string | null
  doneStages: string[]
  thinkingChars: number
  elapsed: number
  tool: string | null
}

const IDLE: Run = {
  stage: null,
  doneStages: [],
  thinkingChars: 0,
  elapsed: 0,
  tool: null,
}

/** Debounce so triage runs on a settled request, not on every keystroke. */
function useDebounced<T>(source: () => T, delay: number): () => T {
  const [value, setValue] = createSignal(source())
  createEffect(() => {
    const next = source()
    const timer = setTimeout(() => setValue(() => next), delay)
    return () => clearTimeout(timer)
  })
  return value
}

export default function App() {
  const [status, { refetch }] = createResource(daemonStatus)
  const [model, setModel] = createSignal("")
  const [enhance, setEnhance] = createSignal(true)
  // Off by default: a measured one-word answer cost 40.5s with reasoning on and
  // 1.0s with it off, and the scratchpad is never shown.
  const [reasoning, setReasoning] = createSignal(false)
  const [draft, setDraft] = createSignal("")

  // Remembered between launches so the folder is chosen once, not every session.
  const FOLDER_KEY = "disco-code.project-root"
  const [projectRoot, setProjectRoot] = createSignal<string | null>(
    localStorage.getItem(FOLDER_KEY),
  )
  const [folderError, setFolderError] = createSignal("")

  createEffect(() => {
    const root = projectRoot()
    if (root) localStorage.setItem(FOLDER_KEY, root)
    else localStorage.removeItem(FOLDER_KEY)
  })

  async function openFolder() {
    setFolderError("")
    try {
      const picked = await chooseProjectFolder()
      // Null means the dialog was dismissed, which must not clear a folder the
      // user already had open.
      if (picked) setProjectRoot(picked)
    } catch (error) {
      setFolderError(error instanceof Error ? error.message : String(error))
    }
  }

  const restored = loadChats()
  const [chats, setChats] = createSignal<Chat[]>(
    restored.length > 0 ? restored : [emptyChat()],
  )
  const [activeId, setActiveId] = createSignal(chats()[0]!.id)

  // Keyed by chat: conversations run independently, so a turn started in one
  // must not block or cancel a turn in another.
  const [runs, setRuns] = createSignal<Record<string, Run>>({})

  const [themeId, setThemeId] = createSignal(DEFAULT_THEME_ID)
  const [triage, setTriage] = createSignal<Triage | null>(null)

  let transcriptRef: HTMLDivElement | undefined

  const active = (): Chat => chats().find((c) => c.id === activeId()) ?? chats()[0]!
  const entries = (): Entry[] => active().entries
  const isRunning = (id: string): boolean => runs()[id] !== undefined
  const runningHere = (): boolean => isRunning(activeId())
  const run = (): Run => runs()[activeId()] ?? IDLE

  onMount(() => {
    const theme = THEMES.find((t) => t.id === DEFAULT_THEME_ID)
    if (theme) applyTheme(theme, true)
  })

  createEffect(() => {
    const theme = THEMES.find((t) => t.id === themeId())
    if (theme) applyTheme(theme, true)
  })

  createEffect(() => saveChats(chats()))

  // Default to the first model that can actually drive the agent loop.
  createEffect(() => {
    const usable = status()?.models.find((m) => m.usable)
    if (usable && !model()) setModel(usable.id)
  })

  // Triage is deterministic and free, so the routing decision is shown while
  // the request is still being typed rather than after minutes of generation.
  const settled = useDebounced(draft, 220)
  createEffect(() => {
    const request = settled().trim()
    if (!request || !enhance()) {
      setTriage(null)
      return
    }
    void triageRequest(request).then(setTriage).catch(() => setTriage(null))
  })

  const scrollDown = () => {
    queueMicrotask(() => {
      if (transcriptRef) transcriptRef.scrollTop = transcriptRef.scrollHeight
    })
  }

  const patchRun = (id: string, edit: (run: Run) => Run) => {
    setRuns((prev) => (prev[id] ? { ...prev, [id]: edit(prev[id]!) } : prev))
  }

  /** Rewrites one chat in place, leaving the others untouched. */
  const patch = (id: string, edit: (chat: Chat) => Chat) => {
    setChats((prev) => prev.map((chat) => (chat.id === id ? edit(chat) : chat)))
  }

  const push = (id: string, entry: Entry) => {
    patch(id, (chat) => ({
      ...chat,
      entries: [...chat.entries, entry],
      updatedAt: Date.now(),
    }))
    if (id === activeId()) scrollDown()
  }

  const appendToLast = (id: string, text: string, stage: string | null) => {
    patch(id, (chat) => {
      const next = [...chat.entries]
      const last = next[next.length - 1]
      if (last && last.role === "assistant" && last.stage === (stage ?? undefined)) {
        next[next.length - 1] = { ...last, text: last.text + text }
      } else {
        next.push({ role: "assistant", text, stage: stage ?? undefined })
      }
      return { ...chat, entries: next, updatedAt: Date.now() }
    })
    if (id === activeId()) scrollDown()
  }

  const startChat = () => {
    const chat = emptyChat()
    setChats((prev) => [chat, ...prev])
    setActiveId(chat.id)
    setDraft("")
    scrollDown()
  }

  const openChat = (id: string) => {
    setActiveId(id)
    scrollDown()
  }

  const removeChat = (id: string) => {
    // A running turn writes into its originating chat, so that chat must not
    // be removed out from under it.
    if (isRunning(id)) return
    setChats((prev) => {
      const next = prev.filter((c) => c.id !== id)
      return next.length > 0 ? next : [emptyChat()]
    })
    if (activeId() === id) setActiveId(chats()[0]!.id)
  }

  const submit = async () => {
    const request = draft().trim()
    const id = activeId()
    // Only this chat has to be idle. Other chats may be mid-turn.
    if (!request || isRunning(id) || !model()) return

    patch(id, (chat) => ({
      ...chat,
      title: chat.entries.length === 0 ? titleFrom(request) : chat.title,
      entries: [...chat.entries, { role: "user", text: request }],
      updatedAt: Date.now(),
    }))

    setDraft("")
    setRuns((prev) => ({ ...prev, [id]: { ...IDLE } }))

    const startedAt = Date.now()
    const ticker = setInterval(() => {
      patchRun(id, (current) => ({
        ...current,
        elapsed: Math.round((Date.now() - startedAt) / 1000),
      }))
    }, 1000)
    scrollDown()

    const onEvent = (event: TurnEvent) => {
      switch (event.kind) {
        case "stage_start":
          patchRun(id, (current) => ({
            ...current,
            doneStages: current.stage
              ? [...current.doneStages, current.stage]
              : current.doneStages,
            stage: event.stage,
          }))
          break
        case "text":
          appendToLast(id, event.text, runs()[id]?.stage ?? null)
          break
        case "thinking":
          // Reasoning is deliberately not rendered as answer text: presenting a
          // model's scratchpad as its conclusion is how wrong answers look
          // confident. Its size is still reported, as proof of progress.
          patchRun(id, (current) => ({
            ...current,
            thinkingChars: current.thinkingChars + event.text.length,
          }))
          break
        case "tool_start":
          patchRun(id, (current) => ({ ...current, tool: event.name }))
          push(id, {
            role: "tool",
            text: `${event.name}: ${event.summary}`,
            ok: true,
          })
          break
        case "tool_end":
          patchRun(id, (current) => ({ ...current, tool: null }))
          if (!event.ok) {
            push(id, {
              role: "tool",
              text: `${event.name} failed: ${event.detail}`,
              ok: false,
            })
          }
          break
        case "failed":
          push(id, { role: "error", text: event.message })
          break
        case "done":
          break
        case "cancelled":
          push(id, { role: "error", text: "Stopped." })
          break
      }
    }

    try {
      await sendPrompt(
        id,
        request,
        model(),
        enhance(),
        reasoning(),
        projectRoot(),
        onEvent,
      )
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      push(id, { role: "error", text: message })
    } finally {
      clearInterval(ticker)
      setRuns((prev) => {
        const next = { ...prev }
        delete next[id]
        return next
      })
      scrollDown()
    }
  }

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault()
      void submit()
    }
  }

  return (
    <div class="app">
      <aside class="sidebar">
        <div class="brand">
          <span class="brand-dot" />
          <span>Disco Code</span>
        </div>

        <div class="chats">
          <div class="chats-head">
            <div class="section-label" style={{ margin: 0 }}>
              Chats
            </div>
            <button class="linkbtn" onClick={startChat}>
              New
            </button>
          </div>
          <div class="chats-list">
            <For each={chats()}>
              {(chat) => (
                <div
                  class={`chat-row ${chat.id === activeId() ? "active" : ""}`}
                  onClick={() => openChat(chat.id)}
                  title={chat.title}
                >
                  <Show when={isRunning(chat.id)}>
                    <span class="spinner" style={{ margin: 0 }} />
                  </Show>
                  <span class="chat-title">{chat.title}</span>
                  <button
                    class="chat-del"
                    title="Delete chat"
                    aria-label={`Delete ${chat.title}`}
                    disabled={isRunning(chat.id)}
                    onClick={(event) => {
                      event.stopPropagation()
                      removeChat(chat.id)
                    }}
                  >
                    &times;
                  </button>
                </div>
              )}
            </For>
          </div>
        </div>

        <div>
          <div class="section-label">Daemon</div>
          <div class="status">
            <span
              class={`status-dot ${status()?.reachable ? "up" : "down"}`}
              aria-hidden="true"
            />
            <span>{status()?.host ?? "checking..."}</span>
          </div>
          <Show when={status() && !status()!.reachable}>
            <div class="hint" style={{ "margin-top": "9px" }}>
              {status()!.detail}
            </div>
          </Show>
        </div>

        <div>
          <div class="section-label">Model</div>
          <select
            value={model()}
            onChange={(event) => setModel(event.currentTarget.value)}
            disabled={!status()?.reachable}
          >
            <For each={status()?.models ?? []}>
              {(entry: Model) => (
                <option value={entry.id} disabled={!entry.usable}>
                  {entry.id}
                  {entry.usable ? "" : " - no tool support"}
                </option>
              )}
            </For>
          </select>
          <Show when={status()?.models.find((m) => m.id === model())}>
            {(current) => (
              <div class="status" style={{ "margin-top": "7px" }}>
                {current().context.toLocaleString()} ctx
                {current().thinking ? " - reasoning" : ""}
                {current().vision ? " - vision" : ""}
              </div>
            )}
          </Show>
          {/* Embedding models are named but never offered: they index text and
              cannot answer, so listing one as a disabled chat model would
              imply it might become selectable. */}
          <Show when={(status()?.embedding_models.length ?? 0) > 0}>
            <div class="aside-note">
              Indexing only: <code>{status()!.embedding_models.join(", ")}</code>
            </div>
          </Show>
        </div>

        <div>
          <div class="section-label">Project folder</div>
          {/* Choosing a folder is what enables writing at all: the model can
              create and change files inside it and nowhere else. With no folder
              chosen there is nothing to confine writes to, so it cannot write. */}
          <Show
            when={projectRoot()}
            fallback={
              <div class="aside-note">
                No folder open, so the model can read and search but cannot
                create or change any file.
              </div>
            }
          >
            <div class="folder-path" title={projectRoot()!}>
              {projectRoot()}
            </div>
            <div class="aside-note">
              The model may edit files here, and nowhere else.
            </div>
          </Show>
          <div class="folder-actions">
            <button class="linkbtn" onClick={openFolder}>
              {projectRoot() ? "Change folder" : "Open folder"}
            </button>
            <Show when={projectRoot()}>
              <button class="linkbtn" onClick={() => setProjectRoot(null)}>
                Close
              </button>
            </Show>
          </div>
          <Show when={folderError()}>
            <div class="aside-note bad">{folderError()}</div>
          </Show>
        </div>

        <div>
          <div class="section-label">Behaviour</div>
          <label class="toggle">
            <input
              type="checkbox"
              checked={enhance()}
              onChange={(event) => setEnhance(event.currentTarget.checked)}
            />
            <span class="toggle-copy">
              <b>Multi-pass</b> - plans, works, then checks itself. Off: one
              direct answer.
            </span>
          </label>
          <label class="toggle" style={{ "margin-top": "11px" }}>
            <input
              type="checkbox"
              checked={reasoning()}
              onChange={(event) => setReasoning(event.currentTarget.checked)}
            />
            <span class="toggle-copy">
              <b>Think first</b> - far slower, often 10x. The thinking is never
              shown.
            </span>
          </label>
        </div>

        <Show when={enhance() && triage()}>
          {(current) => (
            <div class="triage">
              <div class="triage-head">
                <span class="section-label" style={{ margin: 0 }}>
                  Plan
                </span>
                <span class={`badge ${current().complexity}`}>
                  {current().complexity}
                </span>
              </div>
              <div class="stage-chips">
                <For each={current().stages}>
                  {(stage) => (
                    <span
                      class={`chip ${
                        run().stage === stage.stage
                          ? "active"
                          : run().doneStages.includes(stage.stage)
                            ? "done"
                            : ""
                      }`}
                    >
                      {stage.stage}
                    </span>
                  )}
                </For>
              </div>
            </div>
          )}
        </Show>

        <div style={{ "margin-top": "auto" }}>
          <div class="section-label">Theme</div>
          <select
            value={themeId()}
            onChange={(event) => setThemeId(event.currentTarget.value)}
          >
            <For each={THEMES}>
              {(theme) => <option value={theme.id}>{theme.name}</option>}
            </For>
          </select>
          <div class="status" style={{ "margin-top": "10px" }}>
            <button class="linkbtn" onClick={() => void refetch()}>
              Rescan models
            </button>
          </div>
        </div>
      </aside>

      <main class="main">
        <div class="transcript" ref={transcriptRef}>
          <Show
            when={entries().length > 0}
            fallback={
              <div class="empty">
                <h1>Everything runs on your machine</h1>
                <p>
                  Inference is served by your local Ollama daemon. Web and code
                  search still reach the network; your code never goes to a
                  hosted model.
                </p>
              </div>
            }
          >
            <For each={entries()}>
              {(entry) => (
                <Show
                  when={entry.role !== "tool"}
                  fallback={
                    <div class={`toolrow ${entry.ok === false ? "bad" : ""}`}>
                      {entry.text}
                    </div>
                  }
                >
                  <div class={`msg ${entry.role}`}>
                    <Show when={entry.stage}>
                      <div class="stage-marker">{entry.stage}</div>
                    </Show>
                    <div class="msg-role">
                      {entry.role === "user"
                        ? "You"
                        : entry.role === "error"
                          ? "Failed"
                          : "Disco Code"}
                    </div>
                    <div class="msg-body">{entry.text}</div>
                  </div>
                </Show>
              )}
            </For>
          </Show>

          <Show when={runningHere()}>
            <div class="status">
              <span class="spinner" />
              <span>
                {run().tool
                  ? `running ${run().tool}...`
                  : run().stage
                    ? `running ${run().stage}...`
                    : "generating locally..."}
                {` ${run().elapsed}s`}
                {run().thinkingChars > 0
                  ? ` - thinking, ${run().thinkingChars.toLocaleString()} chars so far`
                  : ""}
              </span>
            </div>
          </Show>
        </div>

        <div class="composer">
          <textarea
            placeholder={
              status()?.reachable
                ? "Ask for a change. Shift+Enter for a new line."
                : "Start Ollama to begin."
            }
            value={draft()}
            disabled={!status()?.reachable}
            onInput={(event) => setDraft(event.currentTarget.value)}
            onKeyDown={onKeyDown}
          />
          <button
            class={runningHere() ? "iconbtn stop" : "iconbtn send"}
            title={runningHere() ? "Stop generating" : "Send"}
            aria-label={runningHere() ? "Stop generating" : "Send"}
            onClick={() =>
              runningHere() ? void cancelTurn(activeId()) : void submit()
            }
            disabled={
              !status()?.reachable || (!runningHere() && !draft().trim())
            }
          >
            <Show
              when={runningHere()}
              fallback={
                <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
                  <path d="M8 5v14l11-7z" fill="currentColor" />
                </svg>
              }
            >
              <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true">
                <rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" />
              </svg>
            </Show>
          </button>
        </div>
      </main>
    </div>
  )
}
