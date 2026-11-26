import Foundation

/// Arbitrary JSON (tool arguments, event payloads).
public enum JSONValue: Codable, Hashable, Sendable {
    case null, bool(Bool), number(Double), string(String), array([JSONValue]), object([String: JSONValue])

    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let b = try? c.decode(Bool.self) { self = .bool(b) }
        else if let n = try? c.decode(Double.self) { self = .number(n) }
        else if let s = try? c.decode(String.self) { self = .string(s) }
        else if let a = try? c.decode([JSONValue].self) { self = .array(a) }
        else { self = .object(try c.decode([String: JSONValue].self)) }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case .bool(let b): try c.encode(b)
        case .number(let n): try c.encode(n)
        case .string(let s): try c.encode(s)
        case .array(let a): try c.encode(a)
        case .object(let o): try c.encode(o)
        }
    }

    public subscript(key: String) -> JSONValue? {
        if case .object(let o) = self { return o[key] }
        return nil
    }
    public var string: String? { if case .string(let s) = self { return s }; return nil }
    public var bool: Bool? { if case .bool(let b) = self { return b }; return nil }
    public var number: Double? { if case .number(let n) = self { return n }; return nil }
    public var array: [JSONValue]? { if case .array(let a) = self { return a }; return nil }

    /// Compact one-line summary for tool arguments.
    public var summary: String {
        switch self {
        case .object(let o):
            return o.keys.sorted().map { k in
                let v = o[k]!
                let s = v.string ?? v.compactJSON
                return "\(k)=\(s.prefix(60))"
            }.joined(separator: " ")
        default: return compactJSON
        }
    }

    public var compactJSON: String {
        guard let d = try? JSONEncoder().encode(self) else { return "" }
        return String(decoding: d, as: UTF8.self)
    }

    /// Re-decode into a typed value.
    public func decode<T: Decodable>(_ type: T.Type) throws -> T {
        try AliceClient.decoder.decode(T.self, from: JSONEncoder().encode(self))
    }
}

public struct ChatThread: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public var title: String
    public var model: String?
    public var createdAt: Int64
    public var updatedAt: Int64
    public var generating: Bool?

    public var displayTitle: String { title.isEmpty ? "New chat" : title }
}

public struct ToolCall: Codable, Hashable, Sendable {
    public let id: String
    public let name: String
    public let arguments: JSONValue
}

public struct Message: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public let threadId: String
    public let seq: Int64
    public let role: String
    public var content: String
    public var toolCalls: [ToolCall]?
    public var toolCallId: String?
    public var status: String
    public var client: String
    public var model: String?
    public var meta: JSONValue?
    public let createdAt: Int64

    public var isStreaming: Bool { status == "streaming" }
    public var hasToolCalls: Bool { !(toolCalls ?? []).isEmpty }
    /// True when this assistant message ends a turn.
    /// Calls made through MCP are already answered, so the server marks the last message (`meta.final`).
    public var endsTurn: Bool { role == "assistant" && (status != "complete" || meta?["final"]?.bool == true || !hasToolCalls) && status != "streaming" }
}

public struct Attachment: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public var messageId: String?
    public let name: String
    public let mime: String
    public let size: Int64
    public var isImage: Bool { mime.hasPrefix("image/") && mime != "image/svg+xml" }
}

public struct MessagesResponse: Codable, Sendable {
    public let threadId: String
    public let messages: [Message]
    public let attachments: [Attachment]
    public let generating: Bool
}

public struct ProviderInfo: Codable, Hashable, Sendable {
    public let id: String
    public let kind: String
    public let models: [String]
}

public struct ModelsResponse: Codable, Sendable {
    public let `default`: String
    public let models: [String]
    public let providers: [ProviderInfo]
    /// Display names and reasoning efforts (older servers omit it).
    public let catalog: [ModelInfo]?

    public func info(_ id: String) -> ModelInfo? { catalog?.first { $0.id == ModelID.base(id) } }

    /// Short name for chips: "GPT-6.2-Sol" (also for "default"), else the model part of the id.
    public func shortName(_ id: String) -> String {
        let base = ModelID.base(id)
        guard let i = info(base) else { return String(base.split(separator: "/").last ?? "") }
        if let r = i.resolved, let ri = info("\(i.provider)/\(r)") { return ri.name }
        return i.name
    }
}

public struct ModelInfo: Codable, Hashable, Sendable, Identifiable {
    public let id: String
    public let provider: String
    public let model: String
    public let name: String
    public let description: String
    public let efforts: [String]
    public let defaultEffort: String?
    /// The concrete model behind "default", when known.
    public let resolved: String?
}

/// `provider/model#effort` helpers.
public enum ModelID {
    public static func base(_ id: String) -> String { split(id).0 }
    public static func effort(_ id: String) -> String? { split(id).1 }

    public static func split(_ id: String) -> (String, String?) {
        guard let i = id.lastIndex(of: "#") else { return (id, nil) }
        let e = id[id.index(after: i)...]
        guard !e.isEmpty, e.allSatisfy({ $0.isLowercase && $0.isASCII }) else { return (id, nil) }
        return (String(id[..<i]), String(e))
    }

    public static func join(_ base: String, _ effort: String?) -> String {
        guard let effort, !effort.isEmpty else { return base }
        return "\(base)#\(effort)"
    }
}

public struct PostResult: Codable, Sendable {
    public let userMessage: Message
    public let assistantMessage: Message
    public let model: String
}

/// A pending tool approval (from `tool.approval_required`).
public struct Approval: Identifiable, Hashable, Sendable {
    public let id: String
    public let threadId: String
    public let tool: String
    public let arguments: JSONValue
    public let client: String
}

/// Events pushed by the server over SSE.
public enum ServerEvent: Sendable {
    case hello
    case resync
    case threadCreated(ChatThread)
    case threadUpdated(ChatThread)
    case threadDeleted(String)
    case threadsCleared
    case messageCreated(Message)
    case messageDelta(threadId: String, messageId: String, delta: String)
    case messageCompleted(Message)
    case toolStarted(threadId: String, messageId: String)
    /// Carries the tool's message when the server sends it.
    case toolCompleted(threadId: String, message: Message?)
    /// A turn ended, however it ended.
    case turnDone(String)
    case approvalRequired(Approval)
    case approvalResolved(String)
    case configUpdated
    case other(String)

    public static func parse(_ data: String) -> ServerEvent? {
        guard let d = data.data(using: .utf8), let v = try? AliceClient.decoder.decode(JSONValue.self, from: d),
              let type = v["type"]?.string else { return nil }
        let tid = v["thread_id"]?.string ?? ""
        switch type {
        case "hello": return .hello
        case "resync": return .resync
        case "thread.created": return (try? v["thread"]?.decode(ChatThread.self)).map(ServerEvent.threadCreated)
        case "thread.updated": return (try? v["thread"]?.decode(ChatThread.self)).map(ServerEvent.threadUpdated)
        case "thread.deleted": return .threadDeleted(tid)
        case "threads.cleared": return .threadsCleared
        case "message.created": return (try? v["message"]?.decode(Message.self)).map(ServerEvent.messageCreated)
        case "message.completed": return (try? v["message"]?.decode(Message.self)).map(ServerEvent.messageCompleted)
        case "message.delta":
            return .messageDelta(threadId: tid, messageId: v["message_id"]?.string ?? "", delta: v["delta"]?.string ?? "")
        case "tool.started": return .toolStarted(threadId: tid, messageId: v["message_id"]?.string ?? "")
        case "tool.completed": return .toolCompleted(threadId: tid, message: (try? v["message"]?.decode(Message.self)) ?? nil)
        case "turn.done": return .turnDone(tid)
        case "tool.approval_required":
            return .approvalRequired(Approval(
                id: v["approval_id"]?.string ?? "", threadId: tid, tool: v["tool"]?.string ?? "",
                arguments: v["arguments"] ?? .null, client: v["client"]?.string ?? ""))
        case "tool.approval_resolved": return .approvalResolved(v["approval_id"]?.string ?? "")
        case "config.updated": return .configUpdated
        default: return .other(type)
        }
    }
}
