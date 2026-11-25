import AliceKit
import SwiftUI
import WidgetKit

// Widgets read the snapshot the app writes into the App Group; they make no
// network calls and hold no server token.

@main
struct AliceWidgets: WidgetBundle {
    var body: some Widget {
        TodayWidget()
        MonthWidget()
        TasksWidget()
        AgendaWidget()
    }
}

// MARK: - Timeline

struct Entry: TimelineEntry {
    let date: Date
    let snapshot: WidgetSnapshot?
}

struct Provider: TimelineProvider {
    func placeholder(in context: Context) -> Entry { Entry(date: .now, snapshot: .sample) }

    func getSnapshot(in context: Context, completion: @escaping (Entry) -> Void) {
        completion(Entry(date: .now, snapshot: context.isPreview ? (WidgetSnapshot.load() ?? .sample) : WidgetSnapshot.load()))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<Entry>) -> Void) {
        let snap = WidgetSnapshot.load()
        let now = Date.now
        let cal = Calendar.current
        let midnight = cal.date(byAdding: .day, value: 1, to: cal.startOfDay(for: now)) ?? now.addingTimeInterval(86_400)
        // A new entry whenever an event ends, and at midnight; the app reloads
        // the timeline whenever its data changes.
        let times = [now] + (snap?.changes(after: now, until: midnight) ?? []) + [midnight]
        completion(Timeline(entries: times.map { Entry(date: $0, snapshot: snap) }, policy: .atEnd))
    }
}

// MARK: - Look

enum Tone {
    static let lavender = Color(red: 0.72, green: 0.65, blue: 0.96)
    static let pink = Color(red: 0.97, green: 0.62, blue: 0.78)
    static let accent = LinearGradient(colors: [lavender, pink], startPoint: .topLeading, endPoint: .bottomTrailing)

    static func event(_ e: WidgetSnapshot.Event) -> Color {
        if let t = EventColor.tag(e.colorId) { return Color(red: t.rgb.0, green: t.rgb.1, blue: t.rgb.2) }
        let all = EventColor.tags
        let h = e.id.unicodeScalars.reduce(UInt32(5381)) { ($0 &* 33) &+ $1.value }
        let t = all[Int(h % UInt32(all.count))]
        return Color(red: t.rgb.0, green: t.rgb.1, blue: t.rgb.2)
    }

    static let sunday = Color(red: 0.93, green: 0.45, blue: 0.50)
    static let saturday = Color(red: 0.40, green: 0.60, blue: 0.93)

    static func weekday(_ d: Date) -> Color? {
        switch Calendar.current.component(.weekday, from: d) {
        case 1: sunday
        case 7: saturday
        default: nil
        }
    }
}

struct PastelWidgetBackground: View {
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        if scheme == .dark {
            LinearGradient(colors: [Color(red: 0.13, green: 0.11, blue: 0.19), Color(red: 0.10, green: 0.08, blue: 0.14)], startPoint: .top, endPoint: .bottom)
        } else {
            LinearGradient(colors: [Color(red: 0.97, green: 0.95, blue: 1.0), Color(red: 1.0, green: 0.95, blue: 0.98)], startPoint: .topLeading, endPoint: .bottomTrailing)
        }
    }
}

struct SetupHint: View {
    var body: some View {
        VStack(spacing: 6) {
            Image(systemName: "sparkles").font(.title2).foregroundStyle(Tone.accent)
            Text("Open Alice to show your day here").font(.caption).multilineTextAlignment(.center).foregroundStyle(.secondary)
        }
    }
}

private func time(_ d: Date) -> String { d.formatted(date: .omitted, time: .shortened) }

/// "11/25" large with the weekday small beside it.
struct DateBadge: View {
    let date: Date
    var size: CGFloat = 30

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 4) {
            Text(date.formatted(.dateTime.month(.defaultDigits).day()))
                .font(.system(size: size, weight: .bold, design: .rounded))
                .foregroundStyle(Tone.accent)
            Text(date.formatted(.dateTime.weekday(.abbreviated)))
                .font(.system(size: size * 0.4, weight: .bold, design: .rounded))
                .foregroundStyle(Tone.weekday(date) ?? Tone.pink)
        }
        .lineLimit(1)
        .minimumScaleFactor(0.7)
    }
}

/// One line per event: time, then title; later days get a label line.
struct EventLines: View {
    let items: [(label: String?, event: WidgetSnapshot.Event)]

    var body: some View {
        ForEach(items, id: \.event.id) { item in
            if let l = item.label {
                Text(l).font(.system(size: 10, weight: .bold)).foregroundStyle(Tone.lavender).frame(height: 15, alignment: .bottom)
            }
            HStack(spacing: 6) {
                Capsule().fill(Tone.event(item.event)).frame(width: 3.5, height: 14)
                Text(item.event.allDay ? String(localized: "All day") : time(item.event.start))
                    .font(.system(size: 11, weight: .medium).monospacedDigit())
                    .foregroundStyle(.secondary)
                    .frame(width: 42, alignment: .leading)
                Text(item.event.title).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                Spacer(minLength: 0)
            }
            .frame(height: 17)
        }
    }
}

// MARK: - Today

struct TodayWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "alice.today", provider: Provider()) { entry in
            TodayView(entry: entry)
                .containerBackground(for: .widget) { PastelWidgetBackground() }
                .widgetURL(URL(string: "alice://calendar"))
        }
        .configurationDisplayName("Today")
        .description("Your next events from Google Calendar.")
        .supportedFamilies([.systemSmall, .systemMedium, .systemLarge, .accessoryRectangular, .accessoryInline])
    }
}

/// Upcoming events from `now`, with a label on the first event of each later
/// day. `lines` caps events plus labels, so the list always fits.
func upcoming(_ s: WidgetSnapshot, now: Date, lines: Int) -> [(label: String?, event: WidgetSnapshot.Event)] {
    let cal = Calendar.current
    var out: [(String?, WidgetSnapshot.Event)] = []
    var used = 0
    var seen = Set<String>()
    for i in 0..<7 {
        guard let d = cal.date(byAdding: .day, value: i, to: cal.startOfDay(for: now)) else { continue }
        var first = true
        for e in s.events(on: d, after: now) where seen.insert(e.id).inserted {
            let label: String? = first && i > 0 ? (i == 1 ? String(localized: "Tomorrow") : d.formatted(.dateTime.weekday(.abbreviated).month(.abbreviated).day())) : nil
            let cost = label == nil ? 1 : 2
            if used + cost > lines { return out }
            out.append((label, e))
            used += cost
            first = false
        }
    }
    return out
}

struct TodayView: View {
    let entry: Entry
    @Environment(\.widgetFamily) private var family

    var body: some View {
        if let s = entry.snapshot {
            switch family {
            case .accessoryInline: inline(s)
            case .accessoryRectangular: rectangular(s)
            case .systemSmall: small(s)
            case .systemLarge: large(s)
            default: medium(s)
            }
        } else if family == .accessoryInline {
            Text("Alice")
        } else {
            SetupHint()
        }
    }

    private var dateBadge: some View { DateBadge(date: entry.date) }

    private func row(_ e: WidgetSnapshot.Event, compact: Bool = false) -> some View {
        HStack(spacing: 6) {
            Capsule().fill(Tone.event(e)).frame(width: 3.5)
            VStack(alignment: .leading, spacing: 0) {
                Text(e.title).font(.system(size: compact ? 12 : 13, weight: .semibold)).lineLimit(1)
                Text(e.allDay ? String(localized: "All day") : "\(time(e.start)) – \(time(e.end))")
                    .font(.system(size: compact ? 10 : 11).monospacedDigit())
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
        }
        .fixedSize(horizontal: false, vertical: true)
    }

    private func empty(_ text: LocalizedStringKey = "Nothing planned ☕️") -> some View {
        Text(text).font(.caption).foregroundStyle(.secondary)
    }

    private func small(_ s: WidgetSnapshot) -> some View {
        let items = upcoming(s, now: entry.date, lines: 2)
        return VStack(alignment: .leading, spacing: 5) {
            dateBadge
            if items.isEmpty { empty() }
            ForEach(items, id: \.event.id) { item in
                // Later days are labeled, so tomorrow never looks like today.
                if let l = item.label {
                    Text(l).font(.system(size: 10, weight: .bold)).foregroundStyle(Tone.lavender)
                }
                row(item.event, compact: true)
            }
            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func medium(_ s: WidgetSnapshot) -> some View {
        let items = upcoming(s, now: entry.date, lines: 6)
        return HStack(alignment: .top, spacing: 14) {
            VStack(alignment: .leading) {
                dateBadge
                Spacer(minLength: 0)
            }
            VStack(alignment: .leading, spacing: 4) {
                if items.isEmpty { empty() }
                list(items)
                Spacer(minLength: 0)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func large(_ s: WidgetSnapshot) -> some View {
        let items = upcoming(s, now: entry.date, lines: 13)
        return VStack(alignment: .leading, spacing: 8) {
            dateBadge
            if items.isEmpty { empty() }
            list(items)
            Spacer(minLength: 0)
        }
    }

    private func list(_ items: [(label: String?, event: WidgetSnapshot.Event)]) -> some View { EventLines(items: items) }

    private func rectangular(_ s: WidgetSnapshot) -> some View {
        let items = upcoming(s, now: entry.date, lines: 2)
        return VStack(alignment: .leading, spacing: 1) {
            if let first = items.first?.event {
                Text(first.allDay ? String(localized: "All day") : time(first.start)).font(.caption2.weight(.bold))
                Text(first.title).font(.headline).lineLimit(1)
                if let next = items.dropFirst().first?.event {
                    Text("\(next.allDay ? "" : time(next.start) + " ")\(next.title)").font(.caption2).lineLimit(1).foregroundStyle(.secondary)
                }
            } else {
                Text("Nothing planned").font(.headline)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .widgetAccentable()
    }

    private func inline(_ s: WidgetSnapshot) -> some View {
        if let e = upcoming(s, now: entry.date, lines: 1).first?.event {
            return Text("\(e.allDay ? "" : time(e.start) + " ")\(e.title)")
        }
        return Text("Nothing planned")
    }
}

// MARK: - Month

struct MonthWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "alice.month", provider: Provider()) { entry in
            MonthWidgetView(entry: entry)
                .containerBackground(for: .widget) { PastelWidgetBackground() }
                .widgetURL(URL(string: "alice://calendar"))
        }
        .configurationDisplayName("Month")
        .description("This month at a glance, with your next events.")
        .supportedFamilies([.systemSmall, .systemMedium, .systemLarge])
    }
}

struct MonthWidgetView: View {
    let entry: Entry
    @Environment(\.widgetFamily) private var family

    var body: some View {
        let s = entry.snapshot
        switch family {
        case .systemSmall:
            MonthGrid(today: entry.date, snapshot: s, cell: 17, font: 10)
        case .systemLarge:
            VStack(alignment: .leading, spacing: 10) {
                MonthGrid(today: entry.date, snapshot: s, cell: 30, font: 14)
                if let s {
                    let items = upcoming(s, now: entry.date, lines: 5)
                    if items.isEmpty { Text("Nothing planned ☕️").font(.caption).foregroundStyle(.secondary) }
                    VStack(alignment: .leading, spacing: 4) { EventLines(items: items) }
                }
                Spacer(minLength: 0)
            }
        default:
            HStack(alignment: .top, spacing: 14) {
                MonthGrid(today: entry.date, snapshot: s, cell: 17, font: 10).frame(width: 136)
                VStack(alignment: .leading, spacing: 4) {
                    DateBadge(date: entry.date, size: 22)
                    if let s {
                        let items = upcoming(s, now: entry.date, lines: 5)
                        if items.isEmpty { Text("Nothing planned ☕️").font(.caption).foregroundStyle(.secondary) }
                        EventLines(items: items)
                    }
                    Spacer(minLength: 0)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

/// The month around `today`: weekday initials, then the weeks; today is filled,
/// days with events get a dot.
struct MonthGrid: View {
    let today: Date
    let snapshot: WidgetSnapshot?
    let cell: CGFloat
    let font: CGFloat

    var body: some View {
        let cal = Calendar.current
        let month = cal.dateInterval(of: .month, for: today)
        let first = month.map { cal.dateInterval(of: .weekOfYear, for: $0.start)?.start ?? $0.start } ?? today
        let weeks = month.map { m in
            Int((m.end.timeIntervalSince(first) / 86_400 / 7).rounded(.up))
        } ?? 5
        let symbols = cal.veryShortStandaloneWeekdaySymbols
        let order = (0..<7).map { (cal.firstWeekday - 1 + $0) % 7 }
        VStack(alignment: .leading, spacing: 2) {
            Text(today.formatted(.dateTime.month(.wide)))
                .font(.system(size: font + 1, weight: .bold, design: .rounded))
                .foregroundStyle(Tone.accent)
                .padding(.leading, 2)
            Grid(horizontalSpacing: 0, verticalSpacing: cell > 20 ? 4 : 1) {
                GridRow {
                    ForEach(order, id: \.self) { i in
                        Text(symbols[i])
                            .font(.system(size: font - 2, weight: .bold, design: .rounded))
                            .foregroundStyle(i == 0 ? Tone.sunday : i == 6 ? Tone.saturday : .secondary)
                            .frame(maxWidth: .infinity)
                    }
                }
                ForEach(0..<weeks, id: \.self) { w in
                    GridRow {
                        ForEach(0..<7, id: \.self) { d in
                            let day = cal.date(byAdding: .day, value: w * 7 + d, to: first) ?? first
                            dayCell(day, inMonth: month?.contains(day) ?? true, cal: cal)
                        }
                    }
                }
            }
        }
    }

    private func dayCell(_ day: Date, inMonth: Bool, cal: Calendar) -> some View {
        let isToday = cal.isDate(day, inSameDayAs: today)
        let busy = (snapshot?.eventCount(on: day) ?? 0) > 0
        return VStack(spacing: 1) {
            Text(day.formatted(.dateTime.day()))
                .font(.system(size: font, weight: isToday ? .heavy : .semibold, design: .rounded))
                // Proportional digits, a little tighter, so 10–31 read as one number.
                .tracking(-font * 0.06)
                .foregroundStyle(isToday ? AnyShapeStyle(.white) : AnyShapeStyle(Tone.weekday(day) ?? .primary))
                .frame(width: cell, height: cell * 0.82)
                .background { if isToday { Circle().fill(Tone.accent).frame(width: cell * 0.95, height: cell * 0.95) } }
            Circle()
                .fill(busy && !isToday ? Tone.pink : .clear)
                .frame(width: cell > 20 ? 4 : 3, height: cell > 20 ? 4 : 3)
        }
        .opacity(inMonth ? 1 : 0.28)
        .frame(maxWidth: .infinity)
    }
}

// MARK: - Tasks

struct TasksWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "alice.tasks", provider: Provider()) { entry in
            TasksWidgetView(entry: entry)
                .containerBackground(for: .widget) { PastelWidgetBackground() }
                .widgetURL(URL(string: "alice://tasks"))
        }
        .configurationDisplayName("Tasks")
        .description("Your open Google Tasks, soonest first.")
        .supportedFamilies([.systemSmall, .systemMedium, .accessoryRectangular])
    }
}

struct TasksWidgetView: View {
    let entry: Entry
    @Environment(\.widgetFamily) private var family

    var body: some View {
        if let s = entry.snapshot {
            switch family {
            case .accessoryRectangular:
                VStack(alignment: .leading, spacing: 1) {
                    Text("\(s.tasks.count) tasks").font(.caption2.weight(.bold))
                    ForEach(s.tasks.prefix(2)) { Text($0.title).font(.caption).lineLimit(1) }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .widgetAccentable()
            default:
                // Medium shows two columns once one is full.
                let twoColumns = family != .systemSmall && s.tasks.count > 4
                let shown = Array(s.tasks.prefix(twoColumns ? 8 : 4))
                VStack(alignment: .leading, spacing: 6) {
                    HStack {
                        Image(systemName: "checklist").foregroundStyle(Tone.accent)
                        Text("Tasks").font(.system(.subheadline, design: .rounded).weight(.bold))
                        Spacer()
                        Text("\(s.tasks.count)").font(.system(.title3, design: .rounded).weight(.bold)).foregroundStyle(Tone.accent)
                    }
                    if s.tasks.isEmpty {
                        Text("All done! 🌷").font(.caption).foregroundStyle(.secondary)
                    }
                    if twoColumns {
                        HStack(alignment: .top, spacing: 12) {
                            column(Array(shown.prefix(4)), dues: false)
                            column(Array(shown.dropFirst(4)), dues: false)
                        }
                    } else {
                        column(shown, dues: family != .systemSmall)
                    }
                    Spacer(minLength: 0)
                }
            }
        } else if family == .accessoryRectangular {
            Text("Alice")
        } else {
            SetupHint()
        }
    }

    private func column(_ tasks: [WidgetSnapshot.Task], dues: Bool) -> some View {
        TaskLines(tasks: tasks, now: entry.date, dues: dues)
    }
}

/// Open tasks, one line each, with the due day when `dues`.
struct TaskLines: View {
    let tasks: [WidgetSnapshot.Task]
    let now: Date
    var dues = true

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(tasks) { t in
                HStack(spacing: 6) {
                    Circle().strokeBorder(Tone.lavender, lineWidth: 1.5).frame(width: 11, height: 11)
                    Text(t.title).font(.system(size: 12.5)).lineLimit(1)
                    Spacer(minLength: 4)
                    if dues, let d = t.due { due(d) }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func due(_ d: Date) -> some View {
        let cal = Calendar.current
        let late = d < cal.startOfDay(for: now)
        let label = cal.isDateInToday(d) ? String(localized: "Today") : cal.isDateInTomorrow(d) ? String(localized: "Tomorrow") : d.formatted(.dateTime.month(.abbreviated).day())
        return Text(label)
            .font(.system(size: 10, weight: .semibold))
            .foregroundStyle(late ? .red : Tone.lavender)
    }
}

// MARK: - Today + Tasks

struct AgendaWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "alice.agenda", provider: Provider()) { entry in
            AgendaView(entry: entry)
                .containerBackground(for: .widget) { PastelWidgetBackground() }
        }
        .configurationDisplayName("Today + Tasks")
        .description("Your next events and open tasks side by side.")
        .supportedFamilies([.systemMedium, .systemLarge])
    }
}

/// Events on the left, tasks on the right; each half opens its own tab.
struct AgendaView: View {
    let entry: Entry
    @Environment(\.widgetFamily) private var family

    var body: some View {
        if let s = entry.snapshot {
            let large = family == .systemLarge
            HStack(alignment: .top, spacing: 12) {
                Link(destination: URL(string: "alice://calendar")!) {
                    VStack(alignment: .leading, spacing: 4) {
                        DateBadge(date: entry.date, size: 22)
                        let items = upcoming(s, now: entry.date, lines: large ? 14 : 4)
                        if items.isEmpty { Text("Nothing planned ☕️").font(.caption).foregroundStyle(.secondary) }
                        EventLines(items: items)
                        Spacer(minLength: 0)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                Capsule().fill(Tone.lavender.opacity(0.25)).frame(width: 1)
                Link(destination: URL(string: "alice://tasks")!) {
                    VStack(alignment: .leading, spacing: 6) {
                        HStack(spacing: 5) {
                            Image(systemName: "checklist").foregroundStyle(Tone.accent)
                            Text("Tasks").font(.system(.subheadline, design: .rounded).weight(.bold))
                            Spacer()
                            Text("\(s.tasks.count)").font(.system(.headline, design: .rounded).weight(.bold)).foregroundStyle(Tone.accent)
                        }
                        .frame(height: 27)
                        if s.tasks.isEmpty { Text("All done! 🌷").font(.caption).foregroundStyle(.secondary) }
                        TaskLines(tasks: Array(s.tasks.prefix(large ? 14 : 4)), now: entry.date, dues: false)
                        Spacer(minLength: 0)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .foregroundStyle(.primary)
        } else {
            SetupHint()
        }
    }
}

// MARK: - Sample (widget gallery)

extension WidgetSnapshot {
    static var sample: WidgetSnapshot {
        let cal = Calendar.current
        let today = cal.startOfDay(for: .now)
        func at(_ h: Double) -> Date { today.addingTimeInterval(h * 3600) }
        return WidgetSnapshot(
            events: [
                .init(id: "s1", title: "Lunch with Yuki", start: at(12.5), end: at(13.5), allDay: false, colorId: "6", location: nil),
                .init(id: "s2", title: "Yoga", start: at(19), end: at(20), allDay: false, colorId: "2", location: nil),
                .init(id: "s3", title: "Design review", start: at(39), end: at(40), allDay: false, colorId: "7", location: nil),
            ],
            tasks: [
                .init(id: "t1", title: "Pack for Kyoto", due: today, list: "Tasks"),
                .init(id: "t2", title: "Birthday present", due: at(48), list: "Tasks"),
                .init(id: "t3", title: "Milk, eggs, strawberries", due: nil, list: "Tasks"),
            ],
            unread: 2,
            busy: Dictionary(uniqueKeysWithValues: [0, 1, 3, 6].compactMap { cal.date(byAdding: .day, value: $0, to: today) }.map { (GoogleDate.day($0), 1) }))
    }
}
