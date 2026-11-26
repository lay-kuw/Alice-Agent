import AliceKit
import SwiftUI

/// Alice's face in a soft circle (chat avatar, comment bubbles).
struct MascotAvatar: View {
    var size: CGFloat = 36

    var body: some View {
        Image("MascotFace")
            .resizable()
            .scaledToFill()
            .frame(width: size, height: size)
            .clipShape(Circle())
            .overlay(Circle().strokeBorder(.white.opacity(0.7), lineWidth: 2))
            .shadow(color: Palette.lavender.opacity(0.45), radius: 6, y: 2)
            .accessibilityHidden(true)
    }
}

/// Alice large, floating on a cloud of soft color with a few sparkles.
struct MascotHero: View {
    var size: CGFloat = 200
    @State private var float = false

    var body: some View {
        ZStack {
            Circle().fill(Palette.lavender.opacity(0.35)).frame(width: size * 1.05).blur(radius: 30).offset(x: -size * 0.12)
            Circle().fill(Palette.pink.opacity(0.28)).frame(width: size * 0.8).blur(radius: 30).offset(x: size * 0.22, y: size * 0.1)
            Image("Mascot")
                .resizable()
                .scaledToFit()
                .frame(width: size, height: size)
                .shadow(color: Palette.lavender.opacity(0.45), radius: 14, y: 8)
                .offset(y: float ? -5 : 5)
            Sparkle(size: size * 0.13).offset(x: -size * 0.52, y: -size * 0.42)
            Sparkle(size: size * 0.08).offset(x: -size * 0.62, y: -size * 0.22)
            Sparkle(size: size * 0.09).offset(x: size * 0.55, y: size * 0.38)
        }
        .frame(height: size * 1.1)
        .onAppear { withAnimation(.easeInOut(duration: 2.4).repeatForever(autoreverses: true)) { float = true } }
        .accessibilityHidden(true)
    }
}

struct Sparkle: View {
    let size: CGFloat
    @State private var on = false

    var body: some View {
        Image(systemName: "sparkle")
            .font(.system(size: size, weight: .bold))
            .foregroundStyle(.white)
            .shadow(color: Palette.lemon.opacity(0.8), radius: 4)
            .opacity(on ? 1 : 0.45)
            .scaleEffect(on ? 1 : 0.8)
            .onAppear { withAnimation(.easeInOut(duration: 1.6).repeatForever(autoreverses: true).delay(Double.random(in: 0...1))) { on = true } }
    }
}

/// Alice's one-line comments, per screen. The last lines are kept, so they
/// show at once; all screens are asked together when the app opens.
@MainActor
@Observable
final class Blurbs {
    static let shared = Blurbs()
    private static let linesKey = "blurbs.lines"

    /// Settings switch; off means no model calls at all.
    var enabled: Bool = UserDefaults.standard.object(forKey: "blurbs.enabled") as? Bool ?? true {
        didSet { UserDefaults.standard.set(enabled, forKey: "blurbs.enabled") }
    }
    /// Titles, senders and subjects may go to the model too (off: only counts and times).
    var details: Bool = UserDefaults.standard.bool(forKey: "blurbs.details") {
        didSet {
            UserDefaults.standard.set(details, forKey: "blurbs.details")
            asked = [:]
        }
    }
    private(set) var lines: [String: String] = UserDefaults.standard.dictionary(forKey: linesKey) as? [String: String] ?? [:]
    private var asked: [String: String] = [:]

    /// Comments speak Alice's own language (from SOUL), whatever the app's language.
    func line(_ screen: String) -> String? { enabled ? lines[screen] : nil }

    func ask(_ screen: String, facts: String, client: AliceClient) async {
        guard enabled else { return }
        let sig = facts + "|" + Self.slot()
        guard asked[screen] != sig else { return }
        asked[screen] = sig
        if let text = try? await client.blurb(screen: screen, lang: "soul", facts: facts, now: Self.now()) {
            withAnimation(.snappy) { lines[screen] = text }
            UserDefaults.standard.set(lines, forKey: Self.linesKey)
        } else {
            asked[screen] = nil
        }
    }

    /// Every screen at once (on open, on return, when turned on), so a tab already has its line.
    func refreshAll(google: GoogleStore, client: AliceClient) async {
        guard enabled else { return }
        await withTaskGroup(of: Void.self) { g in
            for (screen, facts) in BlurbFacts.all(google) {
                g.addTask { await self.ask(screen, facts: facts, client: client) }
            }
        }
    }

    /// The tone changed: ask again everywhere.
    func reset() {
        asked = [:]
    }

    private static func now() -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd HH:mm (EEEE)"
        return f.string(from: .now)
    }

    /// Lines follow the part of the day (every 3 hours), not every minute.
    static func slot() -> String { String(Calendar.current.component(.hour, from: .now) / 3) }
}

/// What each screen's comment is about.
@MainActor
enum BlurbFacts {
    static func all(_ g: GoogleStore) -> [(String, String)] {
        [("home", home(g)), ("chat", chat()), ("calendar", calendar(g)), ("mail", mail(g)), ("tasks", tasks(g))]
    }

    static func chat() -> String { "Time of day slot: \(Blurbs.slot())" }

    /// Without the details setting, only counts and times are shared.
    private static var details: Bool { Blurbs.shared.details }

    static func home(_ g: GoogleStore) -> String {
        var f: [String] = []
        let events = g.events(on: .now)
        if !events.isEmpty { f.append("Today's events: " + (details ? events.prefix(5).map(line).joined(separator: "; ") : "\(events.count)")) }
        let due = open(g).filter { (offset($0) ?? 1) <= 0 }
        if !due.isEmpty { f.append("To-dos due today or overdue: \(due.count)" + (details ? " (" + due.prefix(3).map(\.title).joined(separator: ", ") + ")" : "")) }
        if g.inboxLoaded { f.append("Unread mail: \(g.inbox.filter(\.unread).count)") }
        return f.joined(separator: "\n")
    }

    static func calendar(_ g: GoogleStore) -> String {
        let cal = Calendar.current
        let today = cal.startOfDay(for: .now)
        var lines: [String] = []
        for i in 0..<7 {
            guard let d = cal.date(byAdding: .day, value: i, to: today) else { continue }
            let es = g.events(on: d)
            if es.isEmpty { continue }
            let day = i == 0 ? "Today" : i == 1 ? "Tomorrow" : d.formatted(.dateTime.weekday(.wide).locale(Locale(identifier: "en")))
            lines.append("\(day): " + (details ? es.prefix(4).map(line).joined(separator: "; ") : "\(es.count) event(s)"))
        }
        return lines.joined(separator: "\n")
    }

    static func mail(_ g: GoogleStore) -> String {
        guard g.inboxLoaded else { return "" }
        let unread = g.inbox.filter(\.unread)
        guard details else { return "Unread: \(unread.count)" }
        return "Unread: \(unread.count)\n" + unread.prefix(4).map { "- \($0.senderName): \($0.displaySubject)" }.joined(separator: "\n")
    }

    static func tasks(_ g: GoogleStore) -> String {
        guard g.tasksLoaded else { return "" }
        let all = g.lists.flatMap(\.tasks)
        let today = all.filter { t in offset(t).map { t.completed ? $0 == 0 : $0 <= 0 } ?? false }
        let overdue = all.filter { !$0.completed && (offset($0) ?? 0) < 0 }.count
        let counts = "Due today: \(today.count), done: \(today.filter(\.completed).count), overdue: \(overdue), open in total: \(all.filter { !$0.completed }.count)"
        guard details else { return counts }
        return counts + "\n" + today.filter { !$0.completed }.prefix(4).map { "- " + $0.title }.joined(separator: "\n")
    }

    private static func open(_ g: GoogleStore) -> [TaskItem] { g.lists.flatMap(\.tasks).filter { !$0.completed } }

    private static func offset(_ t: TaskItem) -> Int? {
        guard let d = t.dueDate else { return nil }
        let cal = Calendar.current
        return cal.dateComponents([.day], from: cal.startOfDay(for: .now), to: cal.startOfDay(for: d)).day
    }

    private static func line(_ e: CalendarEvent) -> String { "\(e.allDay ? "all day" : e.startDate.map(Fmt.time) ?? "") \(e.displayTitle)" }
}

/// Alice (the transparent artwork) beside her comment at the top of each screen.
struct BlurbBubble: View {
    let screen: String
    /// Shown until (or instead of) Alice's own line.
    let fallback: LocalizedStringKey
    var blurbs = Blurbs.shared

    var body: some View {
        if blurbs.enabled {
            HStack(alignment: .center, spacing: 12) {
                Image("Mascot")
                    .resizable()
                    .scaledToFit()
                    .frame(width: 52, height: 52)
                    .shadow(color: Palette.lavender.opacity(0.45), radius: 6, y: 3)
                    .accessibilityHidden(true)
                Group {
                    if let line = blurbs.line(screen) { Text(line) } else { Text(fallback) }
                }
                .font(.subheadline.weight(.medium))
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(BubbleShape().fill(.background.opacity(0.75)))
                .overlay(BubbleShape().stroke(.white.opacity(0.6), lineWidth: 0.5))
                .shadow(color: Palette.lavender.opacity(0.18), radius: 8, y: 3)
                .contentTransition(.opacity)
            }
        }
    }
}

/// A rounded bubble with a small tail on the left.
struct BubbleShape: Shape {
    func path(in r: CGRect) -> Path {
        // One outline (bubble and tail joined), so the edge runs smoothly round the tail.
        let body = Path(roundedRect: r, cornerRadius: 18, style: .continuous)
        var tail = Path()
        tail.move(to: CGPoint(x: r.minX + 6, y: r.midY - 8))
        tail.addQuadCurve(to: CGPoint(x: r.minX - 7, y: r.midY + 1), control: CGPoint(x: r.minX - 1, y: r.midY - 2))
        tail.addQuadCurve(to: CGPoint(x: r.minX + 6, y: r.midY + 7), control: CGPoint(x: r.minX, y: r.midY + 6))
        tail.closeSubpath()
        return body.union(tail)
    }
}

enum Haptics {
    static func light() { UIImpactFeedbackGenerator(style: .light).impactOccurred() }
    static func medium() { UIImpactFeedbackGenerator(style: .medium).impactOccurred() }
    static func success() { UINotificationFeedbackGenerator().notificationOccurred(.success) }
    static func warning() { UINotificationFeedbackGenerator().notificationOccurred(.warning) }
}
