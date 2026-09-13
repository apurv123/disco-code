/**
 * Conversation storage.
 *
 * Several independent conversations are kept at once, and they survive a
 * restart. A coding session is long and interruptible: forcing the user to
 * choose between keeping the thread they are reading and asking an unrelated
 * question is what made a single-transcript window unusable.
 *
 * Storage is the webview's own `localStorage` rather than a file in the Rust
 * core. Transcripts are display state, they are small, and keeping them here
 * means no IPC round trip on every token. The Rust side stays the place where
 * agent behaviour lives, not where chat scrollback lives.
 */

export type Entry = {
  role: "user" | "assistant" | "error" | "tool"
  text: string
  /** Set when the harness produced this text during a named stage. */
  stage?: string
  /** Tool entries only: false once the call has come back with an error. */
  ok?: boolean
}

export type Attachment = {
  /** Original file name shown in the chat. */
  name: string
  /** Safe name of the app-managed local copy. */
  storedName: string
}

export type Chat = {
  id: string
  title: string
  entries: Entry[]
  attachments: Attachment[]
  model: string
  enhance: boolean
  reasoning: boolean
  updatedAt: number
}

const KEY = "disco-code.chats.v1"

/** Cap on retained conversations, oldest evicted first. */
const LIMIT = 100

export function newId(): string {
  // `randomUUID` is unavailable on insecure origins in some webviews, so the
  // id generation cannot depend on it.
  return `c${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`
}

export function emptyChat(): Chat {
  return {
    id: newId(),
    title: "New chat",
    entries: [],
    attachments: [],
    model: "",
    enhance: true,
    reasoning: false,
    updatedAt: Date.now(),
  }
}

/**
 * Derives a chat's label from its opening request.
 *
 * The first thing the user typed is what they will recognise the thread by, so
 * no separate naming step is imposed on them.
 */
export function titleFrom(request: string): string {
  const line = request.trim().split("\n")[0]?.trim() ?? ""
  if (!line) return "New chat"
  return line.length > 42 ? `${line.slice(0, 41)}...` : line
}

/** Reads saved chats, tolerating absent or corrupt storage. */
export function load(): Chat[] {
  let raw: string | null = null
  try {
    raw = localStorage.getItem(KEY)
  } catch {
    // Storage can be unavailable entirely; an in-memory session is still
    // better than a blank screen.
    return []
  }
  if (!raw) return []

  try {
    const parsed: unknown = JSON.parse(raw)
    if (!Array.isArray(parsed)) return []
    return parsed.filter(isChat).map((chat) => ({
      ...chat,
      // Chats saved before attachment support remain valid.
      attachments: Array.isArray(chat.attachments) ? chat.attachments : [],
      // Model and agent behaviour belong to the conversation. Supply the
      // original defaults when loading chats saved before these fields existed.
      model: typeof chat.model === "string" ? chat.model : "",
      enhance: typeof chat.enhance === "boolean" ? chat.enhance : true,
      reasoning: typeof chat.reasoning === "boolean" ? chat.reasoning : false,
    }))
  } catch {
    return []
  }
}

/** Persists chats, newest first. Failure is non-fatal by design. */
export function save(chats: Chat[]): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(chats.slice(0, LIMIT)))
  } catch {
    // A full or disabled store must not take down the conversation in
    // progress, which is held in memory regardless.
  }
}

function isChat(value: unknown): value is Chat {
  if (typeof value !== "object" || value === null) return false
  const chat = value as Partial<Chat>
  return (
    typeof chat.id === "string" &&
    typeof chat.title === "string" &&
    Array.isArray(chat.entries)
  )
}
