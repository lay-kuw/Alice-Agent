import Foundation
import Network
import Observation

/// UI state for one server connection. The server is the source of truth;
/// deletions arrive as events and are applied here. With `cache`, the thread
/// list and recent conversations are kept on the device for offline reading.
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
    /// Scheduled agents; nil until first loaded.
    public private(set) var agents: [AgentJob]?
    public private(set) var minEveryMin: Int64 = 5
    /// An assistant reply finished in a thread other than the open one.
    public var onReply: (@MainActor (ChatThread, Message) -> Void)?
    /// True while the open thread is on screen (set by the chat view).
    public var viewingChat = false
    /// Model chosen before a thread exists.
    public var pendingModel: String?

    private var eventsTask: Task<Void, Never>?
    /// A lost connection is reported once, not on every retry.
    private var offlineReported = false
    /// Set to retry the connection without waiting out the backoff.
    private var kicked = false
    private var pathMonitor: NWPathMonitor?

    /// Offline copy of threads and recent conversations.
    public var cache: OfflineCache?
    private var convos: [String: MessagesResponse] = [:]
    /// Thread `updatedAt` each saved conversation matches.
    private var stamps: [String: Int64] = [:]
    private var saveTask: Task<Void, Never>?

    private struct Saved: Codable {
        var threads: [ChatThread]
        var models: ModelsResponse?
        var convos: [String: MessagesResponse]
        var stamps: [String: Int64]?
    }

    /// Show the last saved threads at once; the server's answer replaces them.
    public func restore() {
        guard let c = cache?.load(Saved.self, "chat") else { return }
        if threads.isEmpty { threads = c.threads }
        models = models ?? c.models
        convos = c.convos
        stamps = c.stamps ?? [:]
    }

    private func saveSoon() {
        guard cache != nil else { return }
        saveTask?.cancel()
        saveTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(1))
            guard !Task.isCancelled, let self, let cache = self.cache else { return }
            // Conversations of the 20 most recent threads.
            let recent = Set(self.threads.prefix(20).map(\.id))
            self.convos = self.convos.filter { recent.contains($0.key) }
            self.stamps = self.stamps.filter { recent.contains($0.key) }
            cache.save(Saved(threads: self.threads, models: self.models, convos: self.convos, stamps: self.stamps), "chat")
        }
    }

    private func remember(_ id: String) {
        guard cache != nil else { return }
        convos[id] = MessagesResponse(threadId: id, messages: messages, attachments: attachments, generating: false)
        stamps[id] = threads.first { $0.id == id }?.updatedAt
        saveSoon()
    }

    /// Keep the most recent conversations readable offline, fetching only those that changed.
    private func saveRecent() async {
        guard cache != nil else { return }
        for t in threads.prefix(10) where t.generating != true && stamps[t.id] != t.updatedAt {
            guard let r = try? await client.messages(t.id), !r.generating else { continue }
            convos[t.id] = r
            stamps[t.id] = t.updatedAt
        }
        saveSoon()
    }

    /// Try to reconnect now (network came back, app returned to the front).
    public func retryNow() {
        if !connected { kicked = true }
    }

    public init(client: AliceClient) {
        self.client = client
    }

    public var currentThread: ChatThread? { threads.first { $0.id == currentThreadId } }
    public var currentApprovals: [Approval] { approvals.filter { $0.threadId == currentThreadId } }
    public var otherApprovals: [Approval] { approvals.filter { $0.threadId != currentThreadId } }

    public func start() {
        if pathMonitor == nil {
            // Reconnect as soon as the network changes instead of waiting out the backoff.
            let m = NWPathMonitor()
            m.pathUpdateHandler = { [weak self] path in
                guard path.status == .satisfied else { return }
                Task { @MainActor in self?.retryNow() }
            }
            m.start(queue: .global(qos: .utility))
            pathMonitor = m
        }
        guard eventsTask == nil else { return }
        eventsTask = Task { [weak self] in await self?.eventLoop() }
    }

    public func stopEvents() {
        eventsTask?.cancel()
        eventsTask = nil
        pathMonitor?.cancel()
        pathMonitor = nil
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
            // Short backoff (1, 2, 4, 8 s), cut short by retryNow().
            let until = Date().addingTimeInterval(TimeInterval(delay))
            while Date() < until, !kicked, !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(250))
            }
            kicked = false
            delay = min(delay * 2, 8)
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
            saveSoon()
            Task { await saveRecent() }
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
        // The saved copy shows at once (and is all there is while offline).
        if let c = convos[id] {
            messages = c.messages
            attachments = c.attachments
            generating = false
        } else {
            messages = []
            attachments = []
        }
        do { try await load(id) } catch {
            if convos[id] == nil || connected { lastError = error.localizedDescription }
        }
    }

    private func load(_ id: String) async throws {
        let r = try await client.messages(id)
        guard currentThreadId == id else { return }
        messages = r.messages
        attachments = r.attachments
        generating = r.generating
        if !r.generating { remember(id) }
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

    // MARK: scheduled agents

    public func loadAgents() async {
        do {
            let r = try await client.agents()
            agents = r.agents
            minEveryMin = r.minEveryMin
        } catch {
            lastError = error.localizedDescription
        }
    }

    @discardableResult
    public func saveAgent(id: String?, _ f: AgentFields) async -> Bool {
        do {
            try await client.saveAgent(id: id, f)
            await loadAgents()
            return true
        } catch {
            lastError = error.localizedDescription
            return false
        }
    }

    public func setAgentEnabled(_ a: AgentJob, _ on: Bool) async {
        if let i = agents?.firstIndex(where: { $0.id == a.id }) { agents?[i].enabled = on }
        do { try await client.setAgentEnabled(a.id, on) } catch { lastError = error.localizedDescription }
        await loadAgents()
    }

    public func deleteAgent(_ a: AgentJob) async {
        agents?.removeAll { $0.id == a.id }
        do { try await client.deleteAgent(a.id) } catch { lastError = error.localizedDescription }
        await loadAgents()
    }

    /// Run now; returns the thread it posted to.
    public func runAgent(_ a: AgentJob) async -> String? {
        do {
            let t = try await client.runAgent(a.id)
            await loadAgents()
            return t
        } catch {
            lastError = error.localizedDescription
            return nil
        }
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
        case .hello: break
        case .other(let type):
            if type == "agents.updated", agents != nil { await loadAgents() }
        case .resync: await refresh()
        case .threadCreated(let t), .threadUpdated(let t):
            upsertThread(t)
            saveSoon()
        case .threadDeleted(let id):
            threads.removeAll { $0.id == id }
            convos[id] = nil
            saveSoon()
            approvals.removeAll { $0.threadId == id }
            if currentThreadId == id { newChat() }
        case .threadsCleared:
            threads = []
            convos = [:]
            saveSoon()
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
            if m.endsTurn, let i = threads.firstIndex(where: { $0.id == m.threadId }) {
                threads[i].generating = false
                if m.role == "assistant", m.threadId != currentThreadId || !viewingChat { onReply?(threads[i], m) }
            }
            guard m.threadId == currentThreadId else { return }
            upsert(m)
            if m.endsTurn {
                generating = false
                remember(m.threadId)
            }
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
