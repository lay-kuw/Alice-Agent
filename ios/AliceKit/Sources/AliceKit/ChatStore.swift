import Foundation
import Observation

/// UI state for one server connection, kept in memory only. The server is
/// the source of truth; deletions arrive as events and are applied here.
@MainActor
@Observable
public final class ChatStore {
    public let client: AliceClient
    public private(set) var threads: [ChatThread] = []
    public private(set) var currentThreadId: String?
    public private(set) var messages: [Message] = []
    public private(set) var attachments: [Attachment] = []
    public private(set) var generating = false
    public private(set) var approvals: [Approval] = []
    public private(set) var models: ModelsResponse?
    public private(set) var connected = false
    public var lastError: String?
    /// Model chosen before a thread exists.
    public var pendingModel: String?

    private var eventsTask: Task<Void, Never>?
    /// A lost connection is reported once, not on every retry.
    private var offlineReported = false

    public init(client: AliceClient) {
        self.client = client
    }

    public var currentThread: ChatThread? { threads.first { $0.id == currentThreadId } }
    public var currentApprovals: [Approval] { approvals.filter { $0.threadId == currentThreadId } }
    public var otherApprovals: [Approval] { approvals.filter { $0.threadId != currentThreadId } }

    public func start() {
        guard eventsTask == nil else { return }
        eventsTask = Task { [weak self] in await self?.eventLoop() }
    }

    public func stopEvents() {
        eventsTask?.cancel()
        eventsTask = nil
        connected = false
    }

    private func eventLoop() async {
        var delay: UInt64 = 1
        while !Task.isCancelled {
            do {
                for try await ev in client.events() {
                    if case .hello = ev {
                        connected = true
                        offlineReported = false
                        delay = 1
                        await refresh()
                        continue
                    }
                    await apply(ev)
                }
            } catch {
                if Task.isCancelled { return }
                reportOffline(error)
            }
            connected = false
            try? await Task.sleep(nanoseconds: delay * 1_000_000_000)
            delay = min(delay * 2, 30)
        }
    }

    /// Reload everything from the server (after connect or a missed event).
    public func refresh() async {
        do {
            async let t = client.threads()
            async let m = client.models()
            // Only replace what changed, so the list does not redraw mid-refresh.
            let list = try await t
            if list != threads { threads = list }
            models = try await m
            if let id = currentThreadId {
                if threads.contains(where: { $0.id == id }) { try await load(id) } else { newChat() }
            }
        } catch {
            reportOffline(error)
        }
    }

    private func reportOffline(_ error: Error) {
        guard !offlineReported else { return }
        offlineReported = true
        lastError = error.localizedDescription
    }

    public func newChat() {
        currentThreadId = nil
        messages = []
        attachments = []
        generating = false
    }

    public func open(_ id: String) async {
        currentThreadId = id
        do { try await load(id) } catch { lastError = error.localizedDescription }
    }

    private func load(_ id: String) async throws {
        let r = try await client.messages(id)
        guard currentThreadId == id else { return }
        messages = r.messages
        attachments = r.attachments
        generating = r.generating
    }

    @discardableResult
    public func ensureThread() async throws -> String {
        if let id = currentThreadId { return id }
        let t = try await client.createThread(model: pendingModel)
        if !threads.contains(where: { $0.id == t.id }) { threads.insert(t, at: 0) }
        currentThreadId = t.id
        messages = []
        attachments = []
        return t.id
    }

    public func send(_ text: String, attachmentIds: [String] = []) async {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty || !attachmentIds.isEmpty, !generating else { return }
        do {
            let id = try await ensureThread()
            let r = try await client.post(id, text: trimmed, attachments: attachmentIds)
            upsert(r.userMessage)
            upsert(r.assistantMessage)
            generating = true
            for i in attachments.indices where attachmentIds.contains(attachments[i].id) {
                attachments[i].messageId = r.userMessage.id
            }
        } catch {
            lastError = error.localizedDescription
        }
    }

    public func upload(name: String, mime: String, data: Data) async -> Attachment? {
        do {
            let id = try await ensureThread()
            let a = try await client.upload(id, name: name, mime: mime, data: data)
            attachments.append(a)
            return a
        } catch {
            lastError = error.localizedDescription
            return nil
        }
    }

    public func stopGeneration() async {
        guard let id = currentThreadId else { return }
        try? await client.stop(id)
    }

    public func rename(_ id: String, to title: String) async {
        do { try await client.renameThread(id, title: title) } catch { lastError = error.localizedDescription }
    }

    public func delete(_ id: String) async {
        do { try await client.deleteThread(id) } catch { lastError = error.localizedDescription }
    }

    public func setModel(_ model: String) async {
        guard let id = currentThreadId else { pendingModel = model; return }
        do { try await client.setThreadModel(id, model: model) } catch { lastError = error.localizedDescription }
    }

    public func resolve(_ approval: Approval, approve: Bool) async {
        approvals.removeAll { $0.id == approval.id }
        try? await client.resolveApproval(approval.id, approve: approve)
    }

    public func toolResult(for callId: String) -> Message? {
        messages.first { $0.role == "tool" && $0.toolCallId == callId }
    }

    public func attachments(for messageId: String) -> [Attachment] {
        attachments.filter { $0.messageId == messageId }
    }

    private func upsert(_ m: Message) {
        if let i = messages.firstIndex(where: { $0.id == m.id }) { messages[i] = m } else { messages.append(m) }
    }

    private func upsertThread(_ t: ChatThread) {
        if let i = threads.firstIndex(where: { $0.id == t.id }) { threads[i] = t } else { threads.insert(t, at: 0) }
        threads.sort { $0.updatedAt > $1.updatedAt }
    }

    public func apply(_ ev: ServerEvent) async {
        switch ev {
        case .hello, .other: break
        case .resync: await refresh()
        case .threadCreated(let t), .threadUpdated(let t): upsertThread(t)
        case .threadDeleted(let id):
            threads.removeAll { $0.id == id }
            approvals.removeAll { $0.threadId == id }
            if currentThreadId == id { newChat() }
        case .threadsCleared:
            threads = []
            approvals = []
            newChat()
        case .messageCreated(let m):
            if let i = threads.firstIndex(where: { $0.id == m.threadId }) {
                threads[i].updatedAt = m.createdAt
                if m.isStreaming { threads[i].generating = true }
                threads.sort { $0.updatedAt > $1.updatedAt }
            } else {
                threads = (try? await client.threads()) ?? threads
            }
            guard m.threadId == currentThreadId else { return }
            upsert(m)
            if m.isStreaming { generating = true }
            if m.role == "user" { attachments = (try? await client.messages(m.threadId).attachments) ?? attachments }
        case .messageDelta(let tid, let mid, let delta):
            guard tid == currentThreadId, let i = messages.firstIndex(where: { $0.id == mid }) else { return }
            messages[i].content += delta
        case .messageCompleted(let m):
            if m.endsTurn, let i = threads.firstIndex(where: { $0.id == m.threadId }) { threads[i].generating = false }
            guard m.threadId == currentThreadId else { return }
            upsert(m)
            if m.endsTurn { generating = false }
        case .toolStarted(let tid, _), .toolCompleted(let tid, _):
            guard tid == currentThreadId else { return }
            if let r = try? await client.messages(tid), currentThreadId == tid { messages = r.messages }
        case .approvalRequired(let a):
            if !approvals.contains(where: { $0.id == a.id }) { approvals.append(a) }
        case .approvalResolved(let id):
            approvals.removeAll { $0.id == id }
        case .configUpdated:
            models = (try? await client.models()) ?? models
        }
    }
}
