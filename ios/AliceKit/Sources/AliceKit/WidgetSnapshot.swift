import Foundation

/// What the widgets show, written by the app into the shared App Group
/// container. Widgets never talk to the server and hold no token.
public struct WidgetSnapshot: Codable, Sendable, Equatable {
    public struct Event: Codable, Sendable, Equatable, Identifiable {
        public let id: String
        public let title: String
        public let start: Date
        public let end: Date
        public let allDay: Bool
        public let colorId: String?
        public let location: String?

        public init(id: String, title: String, start: Date, end: Date, allDay: Bool, colorId: String?, location: String?) {
            (self.id, self.title, self.start, self.end, self.allDay, self.colorId, self.location) = (id, title, start, end, allDay, colorId, location)
        }
    }

    public struct Task: Codable, Sendable, Equatable, Identifiable {
        public let id: String
        public let title: String
        public let due: Date?
        public let list: String

        public init(id: String, title: String, due: Date?, list: String) {
            (self.id, self.title, self.due, self.list) = (id, title, due, list)
        }
    }

    public var updated: Date
    /// Today and the next six days, sorted.
    public var events: [Event]
    /// Open tasks, soonest due first.
    public var tasks: [Task]
    public var unread: Int?
    /// Number of events per local day ("yyyy-MM-dd"), this month and next, for the month grid.
    public var busy: [String: Int]?

    public init(updated: Date = .now, events: [Event] = [], tasks: [Task] = [], unread: Int? = nil, busy: [String: Int]? = nil) {
        self.updated = updated
        self.events = events
        self.tasks = tasks
        self.unread = unread
        self.busy = busy
    }

    public func eventCount(on day: Date) -> Int { busy?[GoogleDate.day(day)] ?? 0 }

    public static let appGroup = "group.local.alice.app"

    private static var url: URL? {
        FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: appGroup)?.appendingPathComponent("widget.json")
    }

    public static func load() -> WidgetSnapshot? {
        guard let url, let d = try? Data(contentsOf: url) else { return nil }
        let dec = JSONDecoder()
        dec.dateDecodingStrategy = .secondsSince1970
        return try? dec.decode(WidgetSnapshot.self, from: d)
    }

    /// False when the App Group is unavailable (e.g. not signed with it).
    @discardableResult
    public func save() -> Bool {
        guard let url = Self.url else { return false }
        let enc = JSONEncoder()
        enc.dateEncodingStrategy = .secondsSince1970
        guard let d = try? enc.encode(self) else { return false }
        // Readable after the first unlock, so widgets can render on the lock screen.
        return (try? d.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])) != nil
    }

    public static func clear() {
        if let url { try? FileManager.default.removeItem(at: url) }
    }

    // MARK: views for the widgets

    /// Events still to come (or running) on `day`, all-day first.
    public func events(on day: Date, after now: Date, calendar: Calendar = .current) -> [Event] {
        let start = calendar.startOfDay(for: day)
        guard let end = calendar.date(byAdding: .day, value: 1, to: start) else { return [] }
        return events.filter { $0.start < end && $0.end > max(start, now) || ($0.allDay && $0.start < end && $0.end > start) }
    }

    /// Times at which the visible list changes (an event ends) between `now` and `until`.
    public func changes(after now: Date, until: Date) -> [Date] {
        Array(Set(events.filter { !$0.allDay }.map(\.end).filter { $0 > now && $0 < until })).sorted()
    }
}
