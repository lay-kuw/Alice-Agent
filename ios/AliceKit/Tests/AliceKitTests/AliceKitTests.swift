import Foundation
import Testing
@testable import AliceKit

@Test func sseLines() {
    #expect(SSE.data(from: "data: {\"a\":1}") == "{\"a\":1}")
    #expect(SSE.data(from: "data:x") == "x")
    #expect(SSE.data(from: ": keep-alive") == nil)
}

@Test func parsesServerEvents() throws {
    let msg = #"{"type":"message.created","thread_id":"t1","message":{"id":"m1","thread_id":"t1","seq":1,"role":"assistant","content":"","status":"streaming","client":"ios","model":"p/m","created_at":1,"tool_calls":[{"id":"c","name":"fs_list","arguments":{"path":"."}}]}}"#
    guard case .messageCreated(let m) = ServerEvent.parse(msg) else { Issue.record("not parsed"); return }
    #expect(m.threadId == "t1" && m.isStreaming && m.toolCalls?.first?.arguments["path"]?.string == ".")
    guard case .messageDelta(_, let mid, let d) = ServerEvent.parse(#"{"type":"message.delta","thread_id":"t1","message_id":"m1","delta":"hi"}"#) else { Issue.record("delta"); return }
    #expect(mid == "m1" && d == "hi")
    guard case .approvalRequired(let a) = ServerEvent.parse(#"{"type":"tool.approval_required","thread_id":"t1","approval_id":"a1","tool":"fs_delete","arguments":{"path":"x"},"client":"web"}"#) else { Issue.record("approval"); return }
    #expect(a.tool == "fs_delete" && a.arguments.summary == "path=x")
    guard case .threadDeleted(let id) = ServerEvent.parse(#"{"type":"thread.deleted","thread_id":"t9","external":[]}"#) else { Issue.record("deleted"); return }
    #expect(id == "t9")
}

@Test func markdownBlocks() {
    let b = MarkdownBlock.parse("# Title\nsome **bold**\n\n- a\n1. b\n```swift\nlet x = 1\n```\n> quote")
    #expect(b == [.heading(level: 1, text: "Title"), .paragraph("some **bold**"), .listItem(marker: "•", text: "a"),
                  .listItem(marker: "1.", text: "b"), .code(language: "swift", code: "let x = 1"), .quote("quote")])
    // Unclosed fences (while streaming) still render as code.
    #expect(MarkdownBlock.parse("```\nprint(1)") == [.code(language: "", code: "print(1)")])
}

@Test func inlineMarkdownDropsUnsafeLinks() {
    let a = MarkdownBlock.inline("[ok](https://e.com) [bad](javascript:alert(1)) <b>raw</b>")
    let links = a.runs.compactMap { $0.link }
    #expect(links == [URL(string: "https://e.com")!])
    #expect(String(a.characters).contains("<b>raw</b>"))
}

/// End-to-end against a running Alice server:
/// ALICE_TEST_URL=http://127.0.0.1:7878 ALICE_TEST_TOKEN=... swift test
@MainActor
@Test(.enabled(if: ProcessInfo.processInfo.environment["ALICE_TEST_URL"] != nil))
func liveServer() async throws {
    let env = ProcessInfo.processInfo.environment
    let client = AliceClient(baseURL: URL(string: env["ALICE_TEST_URL"]!)!, token: env["ALICE_TEST_TOKEN"] ?? "")
    let bad = AliceClient(baseURL: URL(string: env["ALICE_TEST_URL"]!)!, token: "wrong")
    await #expect(throws: AliceError.self) { try await bad.threads() }

    let store = ChatStore(client: client)
    store.start()
    for _ in 0..<50 where !store.connected { try await Task.sleep(nanoseconds: 100_000_000) }
    #expect(store.connected)

    await store.send("Hello from the iPhone client")
    let tid = try #require(store.currentThreadId)
    for _ in 0..<100 where store.generating { try await Task.sleep(nanoseconds: 50_000_000) }
    #expect(!store.generating)
    let answer = try #require(store.messages.last)
    #expect(answer.role == "assistant" && answer.status == "complete" && !answer.content.isEmpty)
    #expect(store.messages.first?.client == "ios")

    // Image upload round trip.
    let png = Data([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
    let att = try #require(await store.upload(name: "a.png", mime: "image/png", data: png))
    #expect(try await client.attachmentData(att.id) == png)

    // Rename and delete propagate back through events.
    await store.rename(tid, to: "From iOS")
    for _ in 0..<50 where store.currentThread?.title != "From iOS" { try await Task.sleep(nanoseconds: 50_000_000) }
    #expect(store.currentThread?.title == "From iOS")
    await store.delete(tid)
    for _ in 0..<50 where store.currentThreadId != nil { try await Task.sleep(nanoseconds: 50_000_000) }
    #expect(store.currentThreadId == nil && store.messages.isEmpty && !store.threads.contains { $0.id == tid })
    store.stopEvents()
}

@Test func decodesGoogleData() throws {
    let ev = #"{"events":[{"id":"e1","summary":"歯医者","start":"2026-11-25T10:00:00+09:00","end":"2026-11-25T11:00:00+09:00","all_day":false,"location":null,"description":null,"color_id":null,"link":null},{"id":"e2","summary":"","start":"2026-11-28","end":"2026-11-29","all_day":true}]}"#
    struct R: Decodable { let events: [CalendarEvent] }
    let e = try AliceClient.decoder.decode(R.self, from: Data(ev.utf8)).events
    #expect(e[0].startDate == ISO8601DateFormatter().date(from: "2026-11-25T01:00:00Z"))
    #expect(e[1].allDay && e[1].displayTitle == "(No title)" && e[1].startDate != nil)

    let mail = #"{"id":"m1","thread_id":"t","from":"\"Bob B\" <bob@example.com>","to":"me","subject":"","date":"Mon","date_ms":1795000000000,"snippet":"hi","unread":true}"#
    let m = try AliceClient.decoder.decode(MailSummary.self, from: Data(mail.utf8))
    #expect(m.senderName == "Bob B" && m.unread && m.displaySubject == "(No subject)" && m.sentAt != nil)
    #expect(MailAddress.address(m.from) == "bob@example.com")
    #expect(MailAddress.name("bob@example.com") == "bob@example.com")

    let tasks = #"{"lists":[{"id":"L1","title":"My Tasks","tasks":[{"id":"t1","title":"Buy","notes":null,"due":"2026-11-25","completed":false,"completed_at":null,"parent":null,"position":"1"}]}]}"#
    struct T: Decodable { let lists: [TaskList] }
    let l = try AliceClient.decoder.decode(T.self, from: Data(tasks.utf8)).lists
    #expect(l[0].tasks[0].dueDate != nil && !l[0].tasks[0].completed)
}

@Test func finalMessageEndsTurn() throws {
    let j = #"{"id":"m","thread_id":"t","seq":1,"role":"assistant","content":"2件","status":"complete","client":"cli","created_at":1,"tool_calls":[{"id":"mcp_1","name":"calendar_list","arguments":{}}],"meta":{"final":true}}"#
    let m = try AliceClient.decoder.decode(Message.self, from: Data(j.utf8))
    #expect(m.endsTurn)
}
