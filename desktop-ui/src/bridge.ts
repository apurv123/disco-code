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

export function daemonStatus(): Promise<DaemonStatus> {
  return invoke<DaemonStatus>("daemon_status")
}

export function triageRequest(request: string): Promise<Triage> {
  return invoke<Triage>("triage_request", { request })
}

export function sendPrompt(
  request: string,
  model: string,
  enhance: boolean,
  onEvent: (event: TurnEvent) => void,
): Promise<void> {
  const channel = new Channel<TurnEvent>()
  channel.onmessage = onEvent
  return invoke<void>("send_prompt", { channel, request, model, enhance })
}
