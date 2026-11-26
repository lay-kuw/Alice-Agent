import Foundation

public struct AliceError: LocalizedError, Sendable {
    public let message: String
    public let status: Int
    public var errorDescription: String? { message }
}

/// Thin client for the Alice server API. All state lives on the server.
public struct AliceClient: Sendable {
    public let baseURL: URL
    let token: String
    let session: URLSession

    public static let decoder: JSONDecoder = {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        return d
    }()

    public init(baseURL: URL, token: String) {
        self.baseURL = baseURL
        self.token = token
        let cfg = URLSessionConfiguration.ephemeral // no disk cache, no cookies
        cfg.requestCachePolicy = .reloadIgnoringLocalCacheData
        cfg.urlCache = nil
        cfg.timeoutIntervalForRequest = 30
        self.session = URLSession(configuration: cfg)
    }

    func request(_ method: String, _ path: String, body: Data? = nil, contentType: String = "application/json") -> URLRequest {
        var r = URLRequest(url: URL(string: baseURL.absoluteString.trimmingCharacters(in: CharacterSet(charactersIn: "/")) + "/api" + path)!)
        r.httpMethod = method
        r.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        if let body {
            r.httpBody = body
            r.setValue(contentType, forHTTPHeaderField: "Content-Type")
        }
        return r
    }

    @discardableResult
    func send(_ method: String, _ path: String, json: [String: Any]? = nil) async throws -> Data {
        let body = try json.map { try JSONSerialization.data(withJSONObject: $0) }
        return try await perform(request(method, path, body: body))
    }

    func perform(_ r: URLRequest) async throws -> Data {
        let (data, resp) = try await session.data(for: r)
        let status = (resp as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(status) else {
            let msg = (try? JSONDecoder().decode([String: String].self, from: data))?["error"]
            if status == 401 { throw AliceError(message: "Unauthorized: check the API token", status: 401) }
            throw AliceError(message: msg ?? "HTTP \(status)", status: status)
        }
        return data
    }

    func get<T: Decodable>(_ path: String, _ type: T.Type) async throws -> T {
        try Self.decoder.decode(T.self, from: try await perform(request("GET", path)))
    }

    // MARK: API

    public func health() async throws -> JSONValue {
        try await get("/health", JSONValue.self)
    }

    public func status() async throws -> JSONValue {
        try await get("/status", JSONValue.self)
    }

    public func threads() async throws -> [ChatThread] {
        struct R: Decodable { let threads: [ChatThread] }
        return try await get("/threads", R.self).threads
    }

    public func createThread(model: String? = nil) async throws -> ChatThread {
        struct R: Decodable { let thread: ChatThread }
        let d = try await send("POST", "/threads", json: ["model": model ?? NSNull()])
        return try Self.decoder.decode(R.self, from: d).thread
    }

    public func renameThread(_ id: String, title: String) async throws {
        try await send("PATCH", "/threads/\(id)", json: ["title": title])
    }

    public func setThreadModel(_ id: String, model: String) async throws {
        try await send("PATCH", "/threads/\(id)", json: ["model": model])
    }

    public func setThreadPinned(_ id: String, _ pinned: Bool) async throws {
        try await send("PATCH", "/threads/\(id)", json: ["pinned": pinned])
    }

    public func deleteThread(_ id: String) async throws {
        try await send("DELETE", "/threads/\(id)")
    }

    public func messages(_ threadId: String) async throws -> MessagesResponse {
        try await get("/threads/\(threadId)/messages", MessagesResponse.self)
    }

    public func post(_ threadId: String, text: String, attachments: [String] = []) async throws -> PostResult {
        let d = try await send("POST", "/threads/\(threadId)/messages", json: ["content": text, "client": "ios", "attachments": attachments])
        return try Self.decoder.decode(PostResult.self, from: d)
    }

    public func stop(_ threadId: String) async throws {
        try await send("POST", "/threads/\(threadId)/stop", json: [:])
    }

    public func upload(_ threadId: String, name: String, mime: String, data: Data) async throws -> Attachment {
        var comps = URLComponents()
        comps.queryItems = [URLQueryItem(name: "name", value: name), URLQueryItem(name: "mime", value: mime)]
        let r = request("POST", "/threads/\(threadId)/attachments?\(comps.percentEncodedQuery ?? "")", body: data, contentType: "application/octet-stream")
        return try Self.decoder.decode(Attachment.self, from: try await perform(r))
    }

    public func attachmentData(_ id: String) async throws -> Data {
        try await perform(request("GET", "/attachments/\(id)"))
    }

    public func resolveApproval(_ id: String, approve: Bool) async throws {
        try await send("POST", "/approvals/\(id)", json: ["approve": approve])
    }

    public func models() async throws -> ModelsResponse {
        try await get("/models", ModelsResponse.self)
    }

    /// Server-sent events. Ends when the connection closes.
    public func events() -> AsyncThrowingStream<ServerEvent, Error> {
        let r = request("GET", "/events")
        let cfg = URLSessionConfiguration.ephemeral
        cfg.timeoutIntervalForRequest = 3600
        cfg.timeoutIntervalForResource = .infinity
        let s = URLSession(configuration: cfg)
        return AsyncThrowingStream { cont in
            let task = Task {
                do {
                    let (bytes, resp) = try await s.bytes(for: r)
                    let status = (resp as? HTTPURLResponse)?.statusCode ?? 0
                    guard status == 200 else { throw AliceError(message: status == 401 ? "Unauthorized" : "HTTP \(status)", status: status) }
                    for try await line in bytes.lines {
                        if let data = SSE.data(from: line), let ev = ServerEvent.parse(data) {
                            cont.yield(ev)
                        }
                    }
                    cont.finish()
                } catch {
                    cont.finish(throwing: error)
                }
            }
            cont.onTermination = { _ in task.cancel() }
        }
    }
}

public enum SSE {
    /// Payload of a `data:` line, or nil for other lines.
    public static func data(from line: String) -> String? {
        guard line.hasPrefix("data:") else { return nil }
        var s = line.dropFirst(5)
        if s.first == " " { s = s.dropFirst() }
        return String(s)
    }
}
