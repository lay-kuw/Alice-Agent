import AliceKit
import SwiftUI
import UserNotifications

/// Local notifications, built on the device from data the app already loads:
/// event reminders and task due dates are scheduled ahead, new mail is noticed
/// on background refresh. Nothing goes through a push service.
@MainActor
enum Notifier {
    enum Kind: String, CaseIterable, Identifiable {
        case calendar, mail, tasks
        var id: String { rawValue }
        var title: LocalizedStringKey {
            switch self {
            case .calendar: "Calendar"
            case .mail: "Mail"
            case .tasks: "Tasks"
            }
        }
    }

    /// Minutes before a timed event.
    static let eventLead: TimeInterval = 10 * 60
    /// All-day events and due tasks ring at this hour on the day.
    static let morningHour = 9
    private static let seenKey = "notify.mailSeen"
    private static let askedKey = "notify.asked"
    private static let center = UNUserNotificationCenter.current()

    /// The connection's store, for re-planning when a setting changes.
    static weak var store: GoogleStore?

    static func enabled(_ k: Kind) -> Bool {
        UserDefaults.standard.object(forKey: "notify.\(k.rawValue)") as? Bool ?? true
    }

    static func set(_ k: Kind, _ on: Bool) {
        UserDefaults.standard.set(on, forKey: "notify.\(k.rawValue)")
        Task {
            if on { await askOnce(force: true) }
            if let store { await update(store) }
        }
    }

    /// Asks for permission the first time (or when a switch is turned on).
    static func askOnce(force: Bool = false) async {
        let d = UserDefaults.standard
        guard force || !d.bool(forKey: askedKey) else { return }
        d.set(true, forKey: askedKey)
        _ = try? await center.requestAuthorization(options: [.alert, .sound, .badge])
    }

    /// Re-plan reminders from what `g` has loaded, and notify new unread mail
    /// while the app is not in front.
    static func update(_ g: GoogleStore) async {
        let auth = await center.notificationSettings().authorizationStatus
        guard auth == .authorized || auth == .provisional || auth == .ephemeral else { return }
        let now = Date.now
        var planned: [(String, Date, UNMutableNotificationContent)] = []
        var prefixes: [String] = []

        // Only replace a kind once its data is loaded, so a cold start keeps what is planned.
        if g.status?.enabled("calendar") == true, !g.loadedWeeks.isEmpty {
            prefixes.append("cal.")
            if enabled(.calendar) {
                for e in g.snapshot(now: now).events {
                    let at = e.allDay ? morning(e.start) : e.start.addingTimeInterval(-eventLead)
                    guard at > now else { continue }
                    let c = content(e.title, body: e.allDay ? String(localized: "All day") : "\(Fmt.time(e.start)) – \(Fmt.time(e.end))" + (e.location.map { " · \($0)" } ?? ""), tab: "calendar")
                    planned.append(("cal.\(e.id).\(Int(e.start.timeIntervalSince1970))", at, c))
                }
            }
        }
        if g.status?.enabled("tasks") == true, g.tasksLoaded {
            prefixes.append("task.")
            if enabled(.tasks) {
                for l in g.lists {
                    for t in l.tasks where !t.completed && !t.id.hasPrefix("new-") {
                        guard let d = t.dueDate else { continue }
                        let at = morning(d)
                        guard at > now else { continue }
                        planned.append(("task.\(t.id).\(t.due ?? "")", at, content(t.title, body: String(localized: "Due today"), tab: "tasks")))
                    }
                }
            }
        }
        // iOS keeps at most 64 pending; the soonest matter most.
        planned.sort { $0.1 < $1.1 }
        planned = Array(planned.prefix(60))
        let want = Set(planned.map(\.0))
        let pending = await center.pendingNotificationRequests().map(\.identifier)
        let stale = pending.filter { id in prefixes.contains { id.hasPrefix($0) } && !want.contains(id) }
        center.removePendingNotificationRequests(withIdentifiers: stale)
        let have = Set(pending)
        for (id, at, c) in planned where !have.contains(id) {
            let comps = Calendar.current.dateComponents([.year, .month, .day, .hour, .minute], from: at)
            try? await center.add(UNNotificationRequest(identifier: id, content: c, trigger: UNCalendarNotificationTrigger(dateMatching: comps, repeats: false)))
        }

        if g.inboxLoaded { await newMail(g) }
    }

    private static func newMail(_ g: GoogleStore) async {
        let d = UserDefaults.standard
        let seen = d.stringArray(forKey: seenKey)
        // Everything on screen counts as seen; remember a bounded list.
        d.set(Array((g.inbox.map(\.id) + (seen ?? [])).prefix(300)), forKey: seenKey)
        guard let seen, enabled(.mail), UIApplication.shared.applicationState != .active else { return }
        let old = Set(seen)
        for m in g.inbox.filter({ $0.unread && !old.contains($0.id) }).prefix(5) {
            let c = content(m.senderName, body: m.snippet, tab: "mail")
            c.subtitle = m.displaySubject
            try? await center.add(UNNotificationRequest(identifier: "mail.\(m.id)", content: c, trigger: nil))
        }
    }

    /// Nothing is left behind after disconnecting.
    static func clear() {
        center.removeAllPendingNotificationRequests()
        center.removeAllDeliveredNotifications()
        UserDefaults.standard.removeObject(forKey: seenKey)
        store = nil
    }

    private static func morning(_ d: Date) -> Date {
        Calendar.current.date(bySettingHour: morningHour, minute: 0, second: 0, of: d) ?? d
    }

    private static func content(_ title: String, body: String, tab: String) -> UNMutableNotificationContent {
        let c = UNMutableNotificationContent()
        c.title = title
        c.body = body
        c.sound = .default
        c.threadIdentifier = tab
        c.userInfo = ["tab": tab]
        return c
    }
}

/// Where a tapped notification wants to go; MainTabs follows it.
@MainActor
@Observable
final class Router {
    static let shared = Router()
    var tab: String?
}

final class AppDelegate: NSObject, UIApplicationDelegate, UNUserNotificationCenterDelegate {
    func application(_ application: UIApplication, didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        UNUserNotificationCenter.current().delegate = self
        return true
    }

    // Reminders show while the app is open too.
    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification, withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner, .list, .sound])
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse, withCompletionHandler completionHandler: @escaping () -> Void) {
        let tab = response.notification.request.content.userInfo["tab"] as? String
        Task { @MainActor in Router.shared.tab = tab }
        completionHandler()
    }
}
