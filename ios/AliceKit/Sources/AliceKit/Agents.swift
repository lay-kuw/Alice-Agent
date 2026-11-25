import Foundation

/// A scheduled agent: an instruction Alice runs at `startAt` and then every
/// `everyMin` minutes (0 = once), replying in a chat thread.
public struct AgentJob: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public var name: String
    public var prompt: String
    /// nil: a new chat is made on the first run.
    public var threadId: String?
    public var threadTitle: String?
    /// Unix ms.
    public var startAt: Int64
    public var everyMin: Int64
    /// Daily times "HH:MM"; when set, `everyMin` is unused.
    public var times: [String]?
    public var enabled: Bool
    public var nextRun: Int64?
    public var lastRun: Int64?
    public var lastStatus: String?
    public var createdAt: Int64

    public var start: Date { Date(timeIntervalSince1970: Double(startAt) / 1000) }
    public var next: Date? { nextRun.map { Date(timeIntervalSince1970: Double($0) / 1000) } }
    public var last: Date? { lastRun.map { Date(timeIntervalSince1970: Double($0) / 1000) } }

    /// The repeat as a count of the largest whole unit.
    public enum Unit: String, CaseIterable, Sendable {
        case once, minutes, hours, days, weeks
        public var minutes: Int64 {
            switch self {
            case .once: 0
            case .minutes: 1
            case .hours: 60
            case .days: 1440
            case .weeks: 10080
            }
        }
    }

    public static func split(_ everyMin: Int64) -> (Unit, Int64) {
        guard everyMin > 0 else { return (.once, 1) }
        for u in [Unit.weeks, .days, .hours] where everyMin % u.minutes == 0 { return (u, everyMin / u.minutes) }
        return (.minutes, everyMin)
    }
}

/// What the app sends to create or change an agent: a repeat (`everyMin`)
/// or daily `times`, whichever is set.
public struct AgentFields: Sendable {
    public var name: String
    public var prompt: String
    public var threadId: String?
    public var everyMin: Int64
    public var times: [String]
    public var enabled: Bool

    public init(name: String, prompt: String, threadId: String?, everyMin: Int64, times: [String], enabled: Bool) {
        (self.name, self.prompt, self.threadId, self.everyMin, self.times, self.enabled) = (name, prompt, threadId, everyMin, times, enabled)
    }

    var json: [String: Any] {
        var j: [String: Any] = ["name": name, "prompt": prompt, "thread_id": threadId ?? NSNull(), "times": times, "enabled": enabled]
        if times.isEmpty { j["every_min"] = everyMin }
        return j
    }
}

/// A scheduled agent's "nothing to report" (the server removes it right after).
public enum AgentReply {
    public static let silent = "NO_UPDATE"

    public static func isQuiet(_ m: Message) -> Bool {
        guard m.client == "agent", m.role == "assistant" else { return false }
        let t = m.content.trimmingCharacters(in: .whitespacesAndNewlines)
        if t.isEmpty { return false }
        // While streaming, any start of the marker is hidden too.
        return (t.hasPrefix(silent) && t.count <= silent.count + 8) || (m.isStreaming && silent.hasPrefix(t))
    }
}

extension AliceClient {
    public func agents() async throws -> (agents: [AgentJob], minEveryMin: Int64) {
        struct R: Decodable { let agents: [AgentJob]; let minEveryMin: Int64? }
        let r = try await get("/agents", R.self)
        return (r.agents, r.minEveryMin ?? 5)
    }

    @discardableResult
    public func saveAgent(id: String?, _ f: AgentFields) async throws -> AgentJob {
        struct R: Decodable { let agent: AgentJob }
        let d = try await send(id == nil ? "POST" : "PATCH", id.map { "/agents/\($0)" } ?? "/agents", json: f.json)
        return try Self.decoder.decode(R.self, from: d).agent
    }

    public func setAgentEnabled(_ id: String, _ on: Bool) async throws {
        try await send("PATCH", "/agents/\(id)", json: ["enabled": on])
    }

    public func deleteAgent(_ id: String) async throws {
        try await send("DELETE", "/agents/\(id)")
    }

    /// Run now; the schedule stays as it is. Returns the thread it posted to.
    public func runAgent(_ id: String) async throws -> String {
        struct R: Decodable { let threadId: String }
        return try Self.decoder.decode(R.self, from: try await send("POST", "/agents/\(id)/run", json: [:])).threadId
    }
}
