import { createEffect, createResource, createSignal, For, Show, onMount } from "solid-js"
import {
  attachDocuments,
  cancelTurn,
  chooseProjectFolder,
  daemonStatus,
  removeAttachment as removeStoredAttachment,
  sendPrompt,
  type Model,
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
  stopping: boolean
}

const IDLE: Run = {
  stage: null,
  doneStages: [],
  thinkingChars: 0,
  elapsed: 0,
  tool: null,
  stopping: false,
}

export default function App() {
  const [status, { refetch }] = createResource(daemonStatus)
  const [draft, setDraft] = createSignal("")
  const [attaching, setAttaching] = createSignal(false)
  const [attachmentError, setAttachmentError] = createSignal("")
  const [rescanning, setRescanning] = createSignal(false)
  const [rescanMessage, setRescanMessage] = createSignal("")
  const [settingsOpen, setSettingsOpen] = createSignal(false)
  const EMBEDDING_KEY = "disco-code.embedding-model"
  const [embeddingModel, setEmbeddingModel] = createSignal(
    localStorage.getItem(EMBEDDING_KEY) ?? "",
  )

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

  const THEME_KEY = "disco-code.theme"
  const [themeId, setThemeId] = createSignal(
    localStorage.getItem(THEME_KEY) ?? DEFAULT_THEME_ID,
  )
  let transcriptRef: HTMLDivElement | undefined

  const active = (): Chat => chats().find((c) => c.id === activeId()) ?? chats()[0]!
  const entries = (): Entry[] => active().entries
  const isRunning = (id: string): boolean => runs()[id] !== undefined
  const runningHere = (): boolean => isRunning(activeId())
  const run = (): Run => runs()[activeId()] ?? IDLE

  onMount(() => {
    const theme =
      THEMES.find((candidate) => candidate.id === themeId()) ??
      THEMES.find((candidate) => candidate.id === DEFAULT_THEME_ID)
    if (theme) applyTheme(theme, true)
  })

  createEffect(() => {
    const theme = THEMES.find((t) => t.id === themeId())
    if (theme) {
      applyTheme(theme, true)
      localStorage.setItem(THEME_KEY, theme.id)
    }
  })

  createEffect(() => saveChats(chats()))

  createEffect(() => {
    const selected = embeddingModel()
    if (selected) localStorage.setItem(EMBEDDING_KEY, selected)
    else localStorage.removeItem(EMBEDDING_KEY)
  })

  // Repair only chats whose saved model cannot drive the agent loop. Each
  // valid per-chat selection remains untouched when status is refreshed.
  createEffect(() => {
    const models = status()?.models
    if (!models) return
    const fallback = models.find((candidate) => candidate.usable)?.id ?? ""
    setChats((previous) => {
      let changed = false
      const next = previous.map((chat) => {
        const valid = models.some(
          (candidate) => candidate.id === chat.model && candidate.usable,
        )
        if (chat.model && valid) return chat
        if (chat.model === fallback) return chat
        changed = true
        return { ...chat, model: fallback }
      })
      return changed ? next : previous
    })
  })

  createEffect(() => {
    const models = status()?.embedding_models
    if (!models) return
    const current = embeddingModel()
    if (models.includes(current)) return
    const preferred = models.find(
      (candidate) =>
        candidate === "nomic-embed-text" ||
        candidate.startsWith("nomic-embed-text:"),
    )
    setEmbeddingModel(preferred ?? models[0] ?? "")
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

  const attachToActive = async () => {
    const id = activeId()
    if (attaching() || isRunning(id) || !embeddingModel()) return
    setAttachmentError("")
    setAttaching(true)
    try {
      const added = await attachDocuments(id)
      if (added.length > 0) {
        patch(id, (chat) => {
          const byStoredName = new Map(
            chat.attachments.map((attachment) => [attachment.storedName, attachment]),
          )
          for (const attachment of added) {
            byStoredName.set(attachment.storedName, attachment)
          }
          return {
            ...chat,
            attachments: [...byStoredName.values()],
            updatedAt: Date.now(),
          }
        })
      }
    } catch (error) {
      setAttachmentError(error instanceof Error ? error.message : String(error))
    } finally {
      setAttaching(false)
    }
  }

  const removeAttachment = async (storedName: string) => {
    const id = activeId()
    if (isRunning(id)) return
    setAttachmentError("")
    try {
      await removeStoredAttachment(id, storedName)
      patch(id, (chat) => ({
        ...chat,
        attachments: chat.attachments.filter(
          (attachment) => attachment.storedName !== storedName,
        ),
        updatedAt: Date.now(),
      }))
    } catch (error) {
      setAttachmentError(error instanceof Error ? error.message : String(error))
    }
  }

  const rescanModels = async () => {
    if (rescanning()) return
    setRescanning(true)
    setRescanMessage("")
    try {
      const next = await refetch()
      setRescanMessage(
        next?.reachable
          ? `Found ${next.models.length} chat model${next.models.length === 1 ? "" : "s"}.`
          : next?.detail ?? "Ollama is not reachable.",
      )
    } catch (error) {
      setRescanMessage(error instanceof Error ? error.message : String(error))
    } finally {
      setRescanning(false)
      setTimeout(() => setRescanMessage(""), 3500)
    }
  }

  const stopActive = () => {
    const id = activeId()
    const current = runs()[id]
    if (!current || current.stopping) return
    patchRun(id, (running) => ({ ...running, stopping: true }))
    void cancelTurn(id).catch((error) => {
      push(id, {
        role: "error",
        text: error instanceof Error ? error.message : String(error),
      })
    })
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
    const chat = active()
    const hasAttachments = chat.attachments.length > 0
    // Only this chat has to be idle. Other chats may be mid-turn.
    if (!request || isRunning(id) || !chat.model) return

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
        chat.model,
        chat.enhance,
        chat.reasoning,
        projectRoot(),
        hasAttachments,
        embeddingModel() || null,
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

        <div class="sidebar-content">
          <Show
            when={!settingsOpen()}
            fallback={
              <div class="settings-pane">
                <div class="settings-head">
                  <button
                    class="linkbtn"
                    aria-label="Back to chats"
                    onClick={() => setSettingsOpen(false)}
                  >
                    ← Back
                  </button>
                  <strong>Settings</strong>
                </div>

                <div>
                  <div class="section-label">Appearance</div>
                  <label class="field-label" for="theme-select">Theme</label>
                  <select
                    id="theme-select"
                    value={themeId()}
                    onChange={(event) => setThemeId(event.currentTarget.value)}
                  >
                    <For each={THEMES}>
                      {(theme) => <option value={theme.id}>{theme.name}</option>}
                    </For>
                  </select>
                </div>

                <div>
                  <div class="section-label">Ollama Daemon</div>
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
                  <div class="section-label">Embeddings</div>
                  <p class="settings-copy">
                    These models vectorize attached documents for local retrieval
                    and never answer chats.
                  </p>
                  <Show
                    when={(status()?.embedding_models.length ?? 0) > 0}
                    fallback={<div class="aside-note">No embedding model detected.</div>}
                  >
                    <select
                      aria-label="Embedding model"
                      value={embeddingModel()}
                      onChange={(event) =>
                        setEmbeddingModel(event.currentTarget.value)
                      }
                    >
                      <For each={status()!.embedding_models}>
                        {(model) => <option value={model}>{model}</option>}
                      </For>
                    </select>
                  </Show>
                </div>
              </div>
            }
          >
            <div class="chats">
              <div class="chats-head">
                <div class="section-label" style={{ margin: 0 }}>Chats</div>
                <button class="linkbtn" onClick={startChat}>New</button>
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
              <div class="section-label">Project folder</div>
              <Show
                when={projectRoot()}
                fallback={
                  <div class="aside-note">
                    No folder open, so the model can read and search but cannot
                    create or change any file.
                  </div>
                }
              >
                <div class="folder-path" title={projectRoot()!}>{projectRoot()}</div>
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

          </Show>
        </div>

        <div class="sidebar-footer">
          <button
            class="rescan-btn"
            disabled={rescanning()}
            title={rescanMessage() || "Rescan Ollama models"}
            onClick={() => void rescanModels()}
          >
            <Show when={rescanning()}>
              <span class="spinner" aria-hidden="true" />
            </Show>
            <span aria-live="polite">
              {rescanning() ? "Rescanning..." : rescanMessage() || "Rescan models"}
            </span>
          </button>
          <button
            class={`settings-btn ${settingsOpen() ? "active" : ""}`}
            title={settingsOpen() ? "Close settings" : "Open settings"}
            aria-label={settingsOpen() ? "Close settings" : "Open settings"}
            aria-pressed={settingsOpen()}
            onClick={() => setSettingsOpen((open) => !open)}
          >
            <svg viewBox="0 0 24 24" width="17" height="17" aria-hidden="true">
              <path
                fill="currentColor"
                d="M19.1 13a7.4 7.4 0 0 0 .05-1 7.4 7.4 0 0 0-.05-1l2.1-1.65-2-3.46-2.54 1.02a7.8 7.8 0 0 0-1.73-1L14.55 3h-4l-.38 2.91a7.8 7.8 0 0 0-1.73 1L5.9 5.89l-2 3.46L6 11a7.4 7.4 0 0 0-.05 1 7.4 7.4 0 0 0 .05 1l-2.1 1.65 2 3.46 2.54-1.02a7.8 7.8 0 0 0 1.73 1l.38 2.91h4l.38-2.91a7.8 7.8 0 0 0 1.73-1l2.54 1.02 2-3.46L19.1 13ZM12.55 15.5a3.5 3.5 0 1 1 0-7 3.5 3.5 0 0 1 0 7Z"
              />
            </svg>
          </button>
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
               {run().stopping
                 ? "stopping..."
                 : run().tool
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
          <Show when={active().attachments.length > 0}>
            <div class="attachments" aria-label="Attached documents">
              <For each={active().attachments}>
                {(attachment) => (
                  <span class="attachment">
                    <span title={attachment.name}>{attachment.name}</span>
                    <button
                      title={`Remove ${attachment.name}`}
                      disabled={runningHere()}
                      onClick={() => void removeAttachment(attachment.storedName)}
                    >
                      ×
                    </button>
                  </span>
                )}
              </For>
            </div>
          </Show>
          <Show when={attachmentError()}>
            <div class="composer-error" aria-live="polite">
              {attachmentError()}
            </div>
          </Show>
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
          <div class="composer-toolbar">
            <div class="composer-options">
              <select
                class="chat-model"
                aria-label="Chat model"
                title="Model used by this chat"
                value={active().model}
                disabled={runningHere() || !status()?.reachable}
                onChange={(event) => {
                  const model = event.currentTarget.value
                  patch(activeId(), (chat) => ({ ...chat, model }))
                }}
              >
                <For each={status()?.models ?? []}>
                  {(entry: Model) => (
                    <option value={entry.id} disabled={!entry.usable}>
                      {entry.id}{entry.usable ? "" : " - no tool support"}
                    </option>
                  )}
                </For>
              </select>
              <label
                class="compact-toggle"
                title="Plans, works, then checks itself. Off: one direct answer."
              >
                <input
                  type="checkbox"
                  checked={active().enhance}
                  disabled={runningHere()}
                  onChange={(event) => {
                    const enhance = event.currentTarget.checked
                    patch(activeId(), (chat) => ({ ...chat, enhance }))
                  }}
                />
                Multi-pass
              </label>
              <label
                class="compact-toggle"
                title="Far slower, often 10x. The thinking is never shown."
              >
                <input
                  type="checkbox"
                  checked={active().reasoning}
                  disabled={runningHere()}
                  onChange={(event) => {
                    const reasoning = event.currentTarget.checked
                    patch(activeId(), (chat) => ({ ...chat, reasoning }))
                  }}
                />
                Think-first
              </label>
            </div>
            <div class="composer-actions">
              <button
                class="iconbtn attach"
                title={
                  embeddingModel()
                    ? "Attach documents for local retrieval"
                    : "Install or select an embedding model in Settings to attach documents"
                }
                aria-label="Attach documents for local retrieval"
                disabled={attaching() || runningHere() || !embeddingModel()}
                onClick={() => void attachToActive()}
              >
                {attaching() ? <span class="spinner" /> : "＋"}
              </button>
              <button
                class={runningHere() ? "iconbtn stop" : "iconbtn send"}
                title={
                  runningHere()
                    ? run().stopping
                      ? "Stopping"
                      : "Stop generating"
                    : "Send"
                }
                aria-label={
                  runningHere()
                    ? run().stopping
                      ? "Stopping"
                      : "Stop generating"
                    : "Send"
                }
                onClick={() => (runningHere() ? stopActive() : void submit())}
                disabled={
                  runningHere()
                    ? run().stopping
                    : !status()?.reachable || !draft().trim() || !active().model
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
          </div>
        </div>
      </main>
    </div>
  )
}
