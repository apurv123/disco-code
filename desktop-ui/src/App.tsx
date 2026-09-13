import { createEffect, createResource, createSignal, For, Show, onMount } from "solid-js"
import {
  cancelTurn,
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

  const restored = loadChats()
  const [chats, setChats] = createSignal<Chat[]>(
    restored.length > 0 ? restored : [emptyChat()],
  )
  const [activeId, setActiveId] = createSignal(chats()[0]!.id)

  // The chat whose turn is in flight, or null. One turn runs at a time: the
  // core cancels any predecessor when a new one starts, so pretending
  // otherwise here would quietly lose a reply.
  const [busyChat, setBusyChat] = createSignal<string | null>(null)

  const [activeStage, setActiveStage] = createSignal<string | null>(null)
  const [doneStages, setDoneStages] = createSignal<string[]>([])
  const [themeId, setThemeId] = createSignal(DEFAULT_THEME_ID)
  const [triage, setTriage] = createSignal<Triage | null>(null)
  // Reasoning text is never shown, but its volume is: a local model can spend
  // minutes in a hidden scratchpad, and a spinner with no numbers behind it is
  // indistinguishable from a hang.
  const [thinkingChars, setThinkingChars] = createSignal(0)
  const [elapsed, setElapsed] = createSignal(0)

  let transcriptRef: HTMLDivElement | undefined

  const active = (): Chat => chats().find((c) => c.id === activeId()) ?? chats()[0]!
  const entries = (): Entry[] => active().entries
  const running = (): boolean => busyChat() !== null
  const runningHere = (): boolean => busyChat() === activeId()

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
    if (busyChat() === id) return
    setChats((prev) => {
      const next = prev.filter((c) => c.id !== id)
      return next.length > 0 ? next : [emptyChat()]
    })
    if (activeId() === id) setActiveId(chats()[0]!.id)
  }

  const submit = async () => {
    const request = draft().trim()
    if (!request || running() || !model()) return

    const id = activeId()
    patch(id, (chat) => ({
      ...chat,
      title: chat.entries.length === 0 ? titleFrom(request) : chat.title,
      entries: [...chat.entries, { role: "user", text: request }],
      updatedAt: Date.now(),
    }))

    setDraft("")
    setBusyChat(id)
    setActiveStage(null)
    setDoneStages([])
    setThinkingChars(0)
    setElapsed(0)
    const startedAt = Date.now()
    const ticker = setInterval(
      () => setElapsed(Math.round((Date.now() - startedAt) / 1000)),
      1000,
    )
    scrollDown()

    const onEvent = (event: TurnEvent) => {
      switch (event.kind) {
        case "stage_start": {
          const previous = activeStage()
          if (previous) setDoneStages((prev) => [...prev, previous])
          setActiveStage(event.stage)
          break
        }
        case "text":
          appendToLast(id, event.text, activeStage())
          break
        case "thinking":
          // Reasoning is deliberately not rendered as answer text: presenting a
          // model's scratchpad as its conclusion is how wrong answers look
          // confident. Its size is still reported, as proof of progress.
          setThinkingChars((prev) => prev + event.text.length)
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
      await sendPrompt(request, model(), enhance(), reasoning(), onEvent)
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      push(id, { role: "error", text: message })
    } finally {
      clearInterval(ticker)
      setBusyChat(null)
      setActiveStage(null)
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
                  <Show when={busyChat() === chat.id}>
                    <span class="spinner" style={{ margin: 0 }} />
                  </Show>
                  <span class="chat-title">{chat.title}</span>
                  <button
                    class="chat-del"
                    title="Delete chat"
                    aria-label={`Delete ${chat.title}`}
                    disabled={busyChat() === chat.id}
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
                        activeStage() === stage.stage
                          ? "active"
                          : doneStages().includes(stage.stage)
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
              )}
            </For>
          </Show>

          <Show when={runningHere()}>
            <div class="status">
              <span class="spinner" />
              <span>
                {activeStage()
                  ? `running ${activeStage()}...`
                  : "generating locally..."}
                {` ${elapsed()}s`}
                {thinkingChars() > 0
                  ? ` - thinking, ${thinkingChars().toLocaleString()} chars so far`
                  : ""}
              </span>
            </div>
          </Show>
        </div>

        <div class="composer">
          <textarea
            placeholder={
              !status()?.reachable
                ? "Start Ollama to begin."
                : running() && !runningHere()
                  ? "Another chat is generating. Stop it, or wait."
                  : "Ask for a change. Shift+Enter for a new line."
            }
            value={draft()}
            disabled={!status()?.reachable}
            onInput={(event) => setDraft(event.currentTarget.value)}
            onKeyDown={onKeyDown}
          />
          <button
            class={running() ? "iconbtn stop" : "iconbtn send"}
            title={running() ? "Stop generating" : "Send"}
            aria-label={running() ? "Stop generating" : "Send"}
            onClick={() => (running() ? void cancelTurn() : void submit())}
            disabled={!status()?.reachable || (!running() && !draft().trim())}
          >
            <Show
              when={running()}
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
