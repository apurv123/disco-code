import { beforeEach, describe, expect, test } from "vitest"
import { emptyChat, load } from "./chats"

const KEY = "disco-code.chats.v1"

describe("chat settings", () => {
  beforeEach(() => localStorage.clear())

  test("new chats use the conversation defaults", () => {
    expect(emptyChat()).toMatchObject({
      model: "",
      enhance: true,
      reasoning: false,
    })
  })

  test("migrates chats saved before per-chat settings", () => {
    localStorage.setItem(
      KEY,
      JSON.stringify([
        {
          id: "old",
          title: "Old chat",
          entries: [],
          updatedAt: 1,
        },
      ]),
    )

    expect(load()[0]).toMatchObject({
      id: "old",
      attachments: [],
      model: "",
      enhance: true,
      reasoning: false,
    })
  })

  test("retains saved per-chat settings", () => {
    localStorage.setItem(
      KEY,
      JSON.stringify([
        {
          id: "configured",
          title: "Configured chat",
          entries: [],
          attachments: [],
          model: "qwen3",
          enhance: false,
          reasoning: true,
          updatedAt: 1,
        },
      ]),
    )

    expect(load()[0]).toMatchObject({
      model: "qwen3",
      enhance: false,
      reasoning: true,
    })
  })
})
