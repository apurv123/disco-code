/**
 * The bridge to the Rust core.
 *
 * opencode's frontend reaches its backend over a generated HTTP client. This
 * app has no server: the same functions the CLI calls are invoked directly over
 * Tauri IPC. That keeps one implementation of the agent's behaviour rather than
 * two that can disagree.
 */

import { invoke, Channel } from "@tauri-apps/api/core"

export type Model = {
  id: string
  name: string
  context: number
  output: number
  tools: boolean
  vision: boolean
  thinking: boolean
  usable: boolean
}

export type DaemonStatus = {
  host: string
  reachable: boolean
  models: Model[]
  /**
   * Embedding-only models, reported apart from `models`.
   *
   * They are never offered as a conversation partner, because no setting makes
   * one able to answer a prompt, but they are named so a pulled model is never
   * silently missing.
   */
  embedding_models: string[]
  detail: string | null
}

export type Stage = { stage: string; directive: string }

export type Triage = {
  complexity: string
  rationale: string
  signals: string[]
  stages: Stage[]
}

export type TurnEvent =
  | { kind: "stage_start"; stage: string; index: number; total: number }
  | { kind: "text"; text: string }
  | { kind: "thinking"; text: string }
  | { kind: "done" }
  | { kind: "failed"; message: string }
  | { kind: "cancelled" }
  | { kind: "tool_start"; name: string; summary: string }
  | { kind: "tool_end"; name: string; ok: boolean; detail: string }

export type Attachment = {
  name: string
  storedName: string
}

export function daemonStatus(): Promise<DaemonStatus> {
  return invoke<DaemonStatus>("daemon_status")
}

export function triageRequest(request: string): Promise<Triage> {
  return invoke<Triage>("triage_request", { request })
}

/** Abandon the turn running in one chat. Safe to call when nothing is running. */
export function cancelTurn(turnId: string): Promise<void> {
  return invoke<void>("cancel_turn", { turnId })
}

/**
 * Ask for the folder the model may edit. Resolves to null if dismissed.
 *
 * Choosing a folder is what enables writing at all, so it is a deliberate,
 * native act rather than a path typed into the page.
 */
export function chooseProjectFolder(): Promise<string | null> {
  return invoke<string | null>("choose_project_folder")
}

export function attachDocuments(chatId: string): Promise<Attachment[]> {
  return invoke<Attachment[]>("attach_documents", { chatId })
}

export function removeAttachment(
  chatId: string,
  storedName: string,
): Promise<void> {
  return invoke<void>("remove_attachment", { chatId, storedName })
}

export function sendPrompt(
  turnId: string,
  request: string,
  model: string,
  enhance: boolean,
  reasoning: boolean,
  projectRoot: string | null,
  hasAttachments: boolean,
  embeddingModel: string | null,
  onEvent: (event: TurnEvent) => void,
): Promise<void> {
  const channel = new Channel<TurnEvent>()
  channel.onmessage = onEvent
  return invoke<void>("send_prompt", {
    channel,
    turnId,
    request,
    model,
    enhance,
    reasoning,
    projectRoot,
    hasAttachments,
    embeddingModel,
  })
}
