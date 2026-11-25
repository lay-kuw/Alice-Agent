import Foundation
import Observation
#if canImport(WidgetKit)
import WidgetKit
#endif

// MARK: - Models

public struct GoogleStatus: Codable, Sendable {
    public let status: String
    public let detail: String
    public let groups: [String: Bool]
    public let email: String?

    public var ready: Bool { status == "ready" }
    public func enabled(_ group: String) -> Bool { groups[group] ?? false }
}

public struct CalendarEvent: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public var summary: String
    public var start: String
    public var end: String
    public var allDay: Bool
    public var location: String?
    public var description: String?
    public var colorId: String?
    public var link: String?

    public var startDate: Date? { GoogleDate.parse(start) }
    public var endDate: Date? { GoogleDate.parse(end) }
    public var displayTitle: String { summary.isEmpty ? "(No title)" : summary }
}

public struct MailSummary: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public let threadId: String?
    public var from: String
    public var to: String?
    public var subject: String
    public var date: String
    public var dateMs: Int64?
    public var snippet: String
    public var unread: Bool

    public var sentAt: Date? { dateMs.map { Date(timeIntervalSince1970: Double($0) / 1000) } }
    /// "Bob <bob@example.com>" -> "Bob"
    public var senderName: String { MailAddress.name(from) }
    public var displaySubject: String { subject.isEmpty ? "(No subject)" : subject }
}

public struct MailMessage: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public let threadId: String?
    public var from: String
    public var to: String?
    public var cc: String?
    public var replyTo: String?
    public var subject: String
    public var date: String
    public var dateMs: Int64?
    public var body: String

    public var sentAt: Date? { dateMs.map { Date(timeIntervalSince1970: Double($0) / 1000) } }
}

public struct TaskItem: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public var title: String
    public var notes: String?
    /// YYYY-MM-DD
    public var due: String?
    public var completed: Bool
    public var parent: String?

    public var dueDate: Date? { due.flatMap { GoogleDate.parse($0) } }
}

public struct TaskList: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public var title: String
    public var tasks: [TaskItem]
}

public enum MailAddress {
    public static func name(_ s: String) -> String {
        if let i = s.firstIndex(of: "<") {
            let n = s[..<i].trimmingCharacters(in: .whitespaces).trimmingCharacters(in: CharacterSet(charactersIn: "\""))
            if !n.isEmpty { return n }
        }
        return address(s)
    }

    public static func address(_ s: String) -> String {
        if let a = s.firstIndex(of: "<"), let b = s.firstIndex(of: ">"), a < b {
            return String(s[s.index(after: a)..<b])
        }
        return s.trimmingCharacters(in: .whitespaces)
    }
}

public enum GoogleDate {
    /// RFC3339 date-time (with or without fractional seconds) or YYYY-MM-DD (local midnight).
    // Formatters are costly to make; parsing with them is thread-safe.
    nonisolated(unsafe) private static let dayFormat: DateFormatter = {
        let f = DateFormatter()
        f.calendar = Calendar(identifier: .gregorian)
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd"
        return f
    }()
    nonisolated(unsafe) private static let iso = ISO8601DateFormatter()
    nonisolated(unsafe) private static let isoFractional: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    public static func parse(_ s: String) -> Date? {
        if s.count == 10 {
            // Local midnight; the time zone may change while the app runs.
            dayFormat.timeZone = .current
            return dayFormat.date(from: s)
        }
        return iso.date(from: s) ?? isoFractional.date(from: s)
    }

    public static func rfc3339(_ d: Date) -> String {
        let f = ISO8601DateFormatter()
        f.timeZone = .current
        return f.string(from: d)
    }

    public static func day(_ d: Date) -> String {
        dayFormat.timeZone = .current
        return dayFormat.string(from: d)
    }
}

// MARK: - Client

extension AliceClient {
    public func googleStatus() async throws -> GoogleStatus {
        try await get("/google", GoogleStatus.self)
    }

    public func events(from: Date, to: Date) async throws -> [CalendarEvent] {
        struct R: Decodable { let events: [CalendarEvent] }
        return try await get("/google/calendar/events?" + query(["from": GoogleDate.rfc3339(from), "to": GoogleDate.rfc3339(to)]), R.self).events
    }

    @discardableResult
    public func createEvent(summary: String, start: String, end: String, location: String?, notes: String?, colorId: String? = nil) async throws -> CalendarEvent {
        struct R: Decodable { let event: CalendarEvent }
        var body: [String: Any] = ["summary": summary, "start": start, "end": end, "time_zone": TimeZone.current.identifier]
        if let colorId { body["color_id"] = colorId }
        if let location, !location.isEmpty { body["location"] = location }
        if let notes, !notes.isEmpty { body["description"] = notes }
        return try Self.decoder.decode(R.self, from: try await send("POST", "/google/calendar/events", json: body)).event
    }

    /// nil goes back to the calendar's own color.
    public func setEventColor(_ id: String, colorId: String?) async throws -> CalendarEvent {
        struct R: Decodable { let event: CalendarEvent }
        return try Self.decoder.decode(R.self, from: try await send("PATCH", "/google/calendar/events/\(escape(id))", json: ["color_id": colorId ?? ""])).event
    }

    public func deleteEvent(_ id: String) async throws {
        try await send("DELETE", "/google/calendar/events/\(escape(id))")
    }

    public func mail(query q: String = "", page: String? = nil) async throws -> (messages: [MailSummary], nextPage: String?) {
        struct R: Decodable { let messages: [MailSummary]; let nextPage: String? }
        var params = ["q": q]
        if let page { params["page"] = page }
        let r = try await get("/google/mail?" + query(params), R.self)
        return (r.messages, r.nextPage)
    }

    public func mailMessage(_ id: String) async throws -> MailMessage {
        struct R: Decodable { let message: MailMessage }
        return try await get("/google/mail/\(escape(id))", R.self).message
    }

    public func markRead(_ id: String, read: Bool = true) async throws {
        try await send("POST", "/google/mail/\(escape(id))/read", json: ["read": read])
    }

    public func sendMail(to: String, cc: String?, subject: String, body: String, replyTo: String?) async throws {
        var b: [String: Any] = ["to": to, "subject": subject, "body": body]
        if let cc, !cc.isEmpty { b["cc"] = cc }
        if let replyTo { b["reply_to_id"] = replyTo }
        try await send("POST", "/google/mail/send", json: b)
    }

    public func taskLists() async throws -> [TaskList] {
        struct R: Decodable { let lists: [TaskList] }
        return try await get("/google/tasks?completed=true", R.self).lists
    }

    @discardableResult
    public func createTask(list: String, title: String, due: String?) async throws -> TaskItem {
        struct R: Decodable { let task: TaskItem }
        var b: [String: Any] = ["title": title]
        if let due { b["due"] = due }
        return try Self.decoder.decode(R.self, from: try await send("POST", "/google/tasks/\(escape(list))", json: b)).task
    }

    @discardableResult
    public func updateTask(list: String, id: String, fields: [String: Any]) async throws -> TaskItem {
        struct R: Decodable { let task: TaskItem }
        return try Self.decoder.decode(R.self, from: try await send("PATCH", "/google/tasks/\(escape(list))/\(escape(id))", json: fields)).task
    }

    public func deleteTask(list: String, id: String) async throws {
        try await send("DELETE", "/google/tasks/\(escape(list))/\(escape(id))")
    }

    private func query(_ p: [String: String]) -> String {
        var c = URLComponents()
        c.queryItems = p.sorted { $0.key < $1.key }.map { URLQueryItem(name: $0.key, value: $0.value) }
        // `+` in offsets must not turn into a space.
        return (c.percentEncodedQuery ?? "").replacingOccurrences(of: "+", with: "%2B")
    }

    private func escape(_ s: String) -> String {
        s.addingPercentEncoding(withAllowedCharacters: .alphanumerics.union(CharacterSet(charactersIn: "-._~"))) ?? s
    }
}

// MARK: - Store

/// Google data for the Calendar, Mail and Tasks tabs. Kept in memory only (no
/// disk cache); the Alice server holds the tokens and caches Google's answers,
/// so repeat loads are instant. Data already on screen is refreshed quietly.
@MainActor
@Observable
public final class GoogleStore {
    public let client: AliceClient
    public private(set) var status: GoogleStatus?
    /// Every event loaded so far, by id.
    public private(set) var eventsById: [String: CalendarEvent] = [:] { didSet { reindex(); snapshotSoon() } }
    /// Events by local start of day (an event spanning days is on each), sorted.
    public private(set) var eventsByDay: [Date: [CalendarEvent]] = [:]
    /// Week starts whose events are loaded.
    public private(set) var loadedWeeks: Set<Date> = []
    private var rangeLoadedAt: Date?
    public private(set) var inbox: [MailSummary] = [] { didSet { snapshotSoon() } }
    public private(set) var inboxLoaded = false
    public private(set) var nextPage: String?
    public private(set) var bodies: [String: MailMessage] = [:]
    public private(set) var lists: [TaskList] = [] { didSet { snapshotSoon() } }
    public private(set) var tasksLoaded = false
    public private(set) var loading: Set<String> = []
    /// Failed actions (save, send, delete): shown as an alert.
    public var lastError: String?
    /// Failed loads: shown as a small banner; what is on screen stays.
    public var loadError: String?

    /// Write snapshots for the widgets (off in tests and previews).
    public var feedsWidgets = false
    private var snapshotTask: Task<Void, Never>?
    private var lastSnapshot: WidgetSnapshot?

    public init(client: AliceClient, feedsWidgets: Bool = false) {
        self.client = client
        self.feedsWidgets = feedsWidgets
    }

    /// Coalesce bursts of changes into one write.
    private func snapshotSoon() {
        guard feedsWidgets else { return }
        snapshotTask?.cancel()
        snapshotTask = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(800))
            guard !Task.isCancelled else { return }
            self?.writeSnapshot()
        }
    }

    /// The widget data as of now: the next seven days of events, open tasks, unread count.
    public func snapshot(now: Date = .now) -> WidgetSnapshot {
        let cal = Calendar.current
        let today = cal.startOfDay(for: now)
        var seen = Set<String>()
        var list: [WidgetSnapshot.Event] = []
        for i in 0..<7 {
            guard let d = cal.date(byAdding: .day, value: i, to: today) else { continue }
            for e in events(on: d) where seen.insert(e.id).inserted {
                guard let s = e.startDate else { continue }
                var end = e.endDate ?? s
                if e.allDay, end <= s { end = cal.date(byAdding: .day, value: 1, to: s) ?? s }
                list.append(.init(id: e.id, title: e.displayTitle, start: s, end: end, allDay: e.allDay, colorId: e.colorId, location: e.location))
            }
        }
        let tasks = lists.flatMap { l in
            l.tasks.filter { !$0.completed && !$0.id.hasPrefix("new-") }.map { WidgetSnapshot.Task(id: $0.id, title: $0.title, due: $0.dueDate, list: l.title) }
        }
        .enumerated()
        .sorted { a, b in
            // Soonest due first; undated after, in list order.
            switch (a.element.due, b.element.due) {
            case let (x?, y?): return x == y ? a.offset < b.offset : x < y
            case (.some, nil): return true
            case (nil, .some): return false
            default: return a.offset < b.offset
            }
        }
        .map(\.element)
        return WidgetSnapshot(updated: now, events: list, tasks: Array(tasks.prefix(20)), unread: inboxLoaded ? inbox.filter(\.unread).count : nil)
    }

    public func writeSnapshot() {
        var s = snapshot()
        // Keep what this run has not loaded (e.g. tasks turned off or not fetched yet).
        if let old = lastSnapshot ?? WidgetSnapshot.load() {
            if !tasksLoaded { s.tasks = old.tasks }
            if s.unread == nil { s.unread = old.unread }
            if eventsById.isEmpty { s.events = old.events.filter { $0.end > .now } }
        }
        var cmp = s
        cmp.updated = lastSnapshot?.updated ?? s.updated
        guard cmp != lastSnapshot else { return }
        lastSnapshot = s
        guard s.save() else { return }
        #if canImport(WidgetKit)
        WidgetCenter.shared.reloadAllTimelines()
        #endif
    }

    /// Runs `body`, tracking `key` in `loading`; false when it failed.
    @discardableResult
    private func run(_ key: String, load: Bool = false, _ body: () async throws -> Void) async -> Bool {
        loading.insert(key)
        defer { loading.remove(key) }
        do {
            try await body()
            if load { loadError = nil }
            return true
        } catch is CancellationError {
            return false
        } catch {
            if (error as? URLError)?.code == .cancelled { return false }
            if load { loadError = error.localizedDescription } else { lastError = error.localizedDescription }
            return false
        }
    }

    public func loadStatus() async {
        await run("status", load: true) { status = try await client.googleStatus() }
    }

    /// Load everything the tabs show first, in parallel (call on connect).
    public func prefetch() async {
        await loadStatus()
        guard let s = status, s.ready else { return }
        let week = Self.startOfWeek(.now)
        // This month's grid plus a week either side covers both calendar views.
        let cal0 = Calendar.current
        let from = cal0.date(byAdding: .day, value: -7, to: Self.startOfWeek(cal0.dateInterval(of: .month, for: .now)?.start ?? week)) ?? week
        let to = cal0.date(byAdding: .day, value: 49, to: from) ?? week
        async let cal: Void = s.enabled("calendar") ? loadRange(from: from, to: to) : ()
        async let mail: Void = s.enabled("gmail") ? loadInbox() : ()
        async let tasks: Void = s.enabled("tasks") ? loadTasks() : ()
        _ = await (cal, mail, tasks)
    }

    // Calendar

    public static func startOfWeek(_ d: Date) -> Date {
        let cal = Calendar.current
        return cal.dateInterval(of: .weekOfYear, for: d)?.start ?? cal.startOfDay(for: d)
    }

    /// Load events in a range. Unless forced, ranges already loaded are only
    /// refreshed when the last load is older than a minute.
    public func loadRange(from: Date, to: Date, force: Bool = false) async {
        let cal = Calendar.current
        var weeks: [Date] = []
        var w = Self.startOfWeek(from)
        while w < to {
            weeks.append(w)
            w = cal.date(byAdding: .day, value: 7, to: w) ?? to
        }
        if !force, weeks.allSatisfy({ loadedWeeks.contains($0) }), let at = rangeLoadedAt, Date().timeIntervalSince(at) < 60 { return }
        await run("events", load: true) {
            let list = try await client.events(from: from, to: to)
            // Replace what was known for this range.
            var next = eventsById.filter { _, e in
                guard let s = e.startDate else { return true }
                return s < from || s >= to
            }
            for e in list { next[e.id] = e }
            eventsById = next
            for w in weeks { loadedWeeks.insert(w) }
            rangeLoadedAt = Date()
        }
    }

    public func events(on day: Date) -> [CalendarEvent] {
        eventsByDay[Calendar.current.startOfDay(for: day)] ?? []
    }

    /// Dates are parsed once here, not on every draw.
    private func reindex() {
        let cal = Calendar.current
        var out: [Date: [(CalendarEvent, Date)]] = [:]
        for e in eventsById.values {
            guard let s = e.startDate else { continue }
            let end = e.endDate ?? s
            var d = cal.startOfDay(for: s)
            // All-day end dates are exclusive; cap long events at about two months.
            for _ in 0..<62 {
                out[d, default: []].append((e, s))
                guard let n = cal.date(byAdding: .day, value: 1, to: d), n < end else { break }
                d = n
            }
        }
        eventsByDay = out.mapValues { list in
            list.sorted { ($0.0.allDay ? 0 : 1, $0.1) < ($1.0.allDay ? 0 : 1, $1.1) }.map(\.0)
        }
    }

    public func addEvent(summary: String, start: Date, end: Date, allDay: Bool, location: String, notes: String, colorId: String? = nil) async -> Bool {
        var ok = false
        await run("save") {
            let e: CalendarEvent
            if allDay {
                let next = Calendar.current.date(byAdding: .day, value: 1, to: end) ?? end
                e = try await client.createEvent(summary: summary, start: GoogleDate.day(start), end: GoogleDate.day(next), location: location, notes: notes, colorId: colorId)
            } else {
                e = try await client.createEvent(summary: summary, start: GoogleDate.rfc3339(start), end: GoogleDate.rfc3339(end), location: location, notes: notes, colorId: colorId)
            }
            eventsById[e.id] = e
            ok = true
        }
        return ok
    }

    /// Change an event's color tag at once; undone if the server says no.
    public func setColor(_ e: CalendarEvent, _ colorId: String?) async {
        guard var cur = eventsById[e.id] else { return }
        let before = cur
        cur.colorId = colorId
        eventsById[e.id] = cur
        let ok = await run("save") {
            let saved = try await client.setEventColor(e.id, colorId: colorId)
            eventsById[e.id] = saved
        }
        if !ok { eventsById[e.id] = before }
    }

    public func deleteEvent(_ e: CalendarEvent) async {
        eventsById[e.id] = nil
        let ok = await run("save") { try await client.deleteEvent(e.id) }
        if !ok { eventsById[e.id] = e }
    }

    // Mail

    public func loadInbox(query: String = "") async {
        await run("mail", load: true) {
            let r = try await client.mail(query: query)
            inbox = r.messages
            nextPage = r.nextPage
            inboxLoaded = true
        }
        // Bodies of the newest messages, so opening them is instant.
        let ids = inbox.prefix(8).map(\.id).filter { bodies[$0] == nil }
        await withTaskGroup(of: MailMessage?.self) { group in
            for id in ids { group.addTask { [client] in try? await client.mailMessage(id) } }
            for await m in group { if let m { bodies[m.id] = m } }
        }
    }

    public func loadMore(query: String = "") async {
        guard let page = nextPage, !loading.contains("more") else { return }
        await run("more", load: true) {
            let r = try await client.mail(query: query, page: page)
            inbox += r.messages.filter { m in !inbox.contains { $0.id == m.id } }
            nextPage = r.nextPage
        }
    }

    public func open(_ id: String) async -> MailMessage? {
        if let m = bodies[id] { return m }
        await run("open", load: true) { bodies[id] = try await client.mailMessage(id) }
        return bodies[id]
    }

    /// Mark read (or unread) at once; undone if the server says no.
    public func setRead(_ id: String, _ read: Bool) async {
        guard let i = inbox.firstIndex(where: { $0.id == id }), inbox[i].unread == read else { return }
        inbox[i].unread = !read
        let ok = await run("read") { try await client.markRead(id, read: read) }
        if !ok, let i = inbox.firstIndex(where: { $0.id == id }) { inbox[i].unread = read }
    }

    public func send(to: String, cc: String, subject: String, body: String, replyTo: String?) async -> Bool {
        var ok = false
        await run("send") {
            try await client.sendMail(to: to, cc: cc, subject: subject, body: body, replyTo: replyTo)
            ok = true
        }
        return ok
    }

    // Tasks

    public func loadTasks() async {
        await run("tasks", load: true) {
            lists = try await client.taskLists()
            tasksLoaded = true
        }
    }

    public func addTask(list: String, title: String, due: Date?) async {
        guard let li = lists.firstIndex(where: { $0.id == list }) else { return }
        // Show it at once; replace with the server's copy.
        let temp = TaskItem(id: "new-\(UUID().uuidString)", title: title, notes: nil, due: due.map(GoogleDate.day), completed: false, parent: nil)
        lists[li].tasks.insert(temp, at: 0)
        let ok = await run("save") {
            let t = try await client.createTask(list: list, title: title, due: due.map(GoogleDate.day))
            if let li = lists.firstIndex(where: { $0.id == list }), let ti = lists[li].tasks.firstIndex(where: { $0.id == temp.id }) { lists[li].tasks[ti] = t }
        }
        if !ok, let li = lists.firstIndex(where: { $0.id == list }) { lists[li].tasks.removeAll { $0.id == temp.id } }
    }

    public func toggle(_ task: TaskItem, in list: String) async {
        guard !task.id.hasPrefix("new-"), let li = lists.firstIndex(where: { $0.id == list }), let ti = lists[li].tasks.firstIndex(where: { $0.id == task.id }) else { return }
        let done = !task.completed
        lists[li].tasks[ti].completed = done // optimistic
        let ok = await run("save") { try await client.updateTask(list: list, id: task.id, fields: ["completed": done]) }
        if !ok, let ti = lists[li].tasks.firstIndex(where: { $0.id == task.id }) { lists[li].tasks[ti].completed = !done }
    }

    public func deleteTask(_ task: TaskItem, in list: String) async {
        guard let li = lists.firstIndex(where: { $0.id == list }) else { return }
        let before = lists[li].tasks
        lists[li].tasks.removeAll { $0.id == task.id }
        let ok = await run("save") { try await client.deleteTask(list: list, id: task.id) }
        if !ok, let li = lists.firstIndex(where: { $0.id == list }) { lists[li].tasks = before }
    }
}
