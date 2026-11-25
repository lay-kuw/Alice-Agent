import AliceKit
import SwiftUI

struct CalendarView: View {
    @Bindable var google: GoogleStore
    @State private var selected = Calendar.current.startOfDay(for: .now)
    /// Week shown in the top row of the scroller.
    @State private var topWeek: Date?
    @AppStorage("calendar.mode") private var mode = "week"
    @State private var adding = false
    @State private var detail: CalendarEvent?
    @State private var confirmDelete: CalendarEvent?

    /// About three years either way; rows are created lazily. Made once, so
    /// the rows keep their identity across redraws.
    @State private var weeks: [Date] = {
        let cal = Calendar.current
        let now = GoogleStore.startOfWeek(.now)
        return (-156...156).compactMap { cal.date(byAdding: .day, value: 7 * $0, to: now) }
    }()
    /// Set while the app itself moves the calendar (today, mode, day swipe).
    @State private var moving = false
    /// The last real position, in case the scroller reports none on the way to a bad jump.
    @State private var lastTop: Date?

    private var rows: Int { mode == "month" ? 6 : 1 }
    private static let rowHeight: CGFloat = 50

    var body: some View {
        NavigationStack {
            GoogleGate(google: google, group: "calendar", title: "Calendar") {
                // The calendar stays put; only the day's events scroll, so the
                // calendar's own scrolling never drags the page.
                VStack(alignment: .leading, spacing: 12) {
                    calendarCard
                    dayHeader
                    ScrollView {
                        dayEvents.padding(.bottom, 30)
                    }
                    .scrollIndicators(.hidden)
                    .refreshable { await load(force: true) }
                }
                .padding(.horizontal)
                .background(PastelBackground())
                .task(id: "\(mode)|\(topWeek?.timeIntervalSince1970 ?? 0)") {
                    // Wait for scrolling to settle before loading.
                    try? await Task.sleep(for: .milliseconds(150))
                    await load(force: false)
                }
            }
            .navigationTitle(focusMonth.formatted(.dateTime.year().month(.wide)))
            .navigationBarTitleDisplayMode(.inline)
            .onAppear {
                #if DEBUG
                if let m = ProcessInfo.processInfo.environment["ALICE_DEV_CALENDAR"] { mode = m }
                if let n = ProcessInfo.processInfo.environment["ALICE_DEV_DAY"].flatMap(Int.init) {
                    selected = Calendar.current.date(byAdding: .day, value: n, to: selected) ?? selected
                }
                #endif
                // Month view starts at the month's first week, week view at this week.
                if topWeek == nil { topWeek = GoogleStore.startOfWeek(mode == "month" ? Self.startOfMonth(selected) : selected) }
            }
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { modeMenu }
                ToolbarItem(placement: .topBarTrailing) {
                    Button { adding = true } label: { Image(systemName: "plus.circle.fill").font(.title3).foregroundStyle(Palette.accent) }
                }
            }
            .sheet(isPresented: $adding) { NewEventSheet(google: google, day: selected) }
            .sheet(item: $detail) { EventDetailSheet(google: google, event: $0) }
            .confirmationDialog("Delete “\(confirmDelete?.displayTitle ?? "")”?", isPresented: Binding(get: { confirmDelete != nil }, set: { if !$0 { confirmDelete = nil } }), titleVisibility: .visible) {
                Button("Delete", role: .destructive) { if let e = confirmDelete { Task { await google.deleteEvent(e) } } }
            }
            .googleErrors(google)
        }
    }

    // MARK: mode

    private var modeMenu: some View {
        Menu {
            Picker("View", selection: Binding(get: { mode }, set: { setMode($0) })) {
                Label("Week", systemImage: Self.icon("week")).tag("week")
                Label("Month", systemImage: Self.icon("month")).tag("month")
            }
            Divider()
            Button("Jump to today", systemImage: "sun.max") { jumpToToday() }
        } label: {
            Image(systemName: Self.icon(mode))
                .font(.system(size: 16, weight: .semibold))
                .foregroundStyle(Palette.lavender)
                .contentTransition(.symbolEffect(.replace))
        }
        .accessibilityLabel(mode == "month" ? "Month view" : "Week view")
    }

    private static func icon(_ mode: String) -> String { mode == "month" ? "calendar" : "rectangle.split.3x1" }

    private func setMode(_ m: String) {
        move {
            mode = m
            let anchor = m == "month" ? Self.startOfMonth(selected) : selected
            topWeek = GoogleStore.startOfWeek(anchor)
        }
    }

    private func jumpToToday() {
        move {
            selected = Calendar.current.startOfDay(for: .now)
            topWeek = GoogleStore.startOfWeek(mode == "month" ? Self.startOfMonth(.now) : .now)
        }
    }

    /// A jump the app makes on purpose; long jumps are otherwise undone (see `guardJump`).
    private func move(_ change: () -> Void) {
        moving = true
        withAnimation(.snappy, change)
        Task {
            try? await Task.sleep(for: .milliseconds(700))
            moving = false
        }
    }

    /// SwiftUI sometimes loses a lazy scroller's position when the rows redraw
    /// (new events, a mode switch) and snaps to the first row, three years back.
    /// Scrolling by hand moves a few weeks per update at most, so a long jump
    /// the app did not ask for is put back.
    private func guardJump(from old: Date?, to new: Date?) -> Bool {
        guard !moving, let old, let new else { return false }
        let weeksApart = abs(Calendar.current.dateComponents([.day], from: old, to: new).day ?? 0) / 7
        guard weeksApart > 12 else { return false }
        var t = Transaction()
        t.disablesAnimations = true
        withTransaction(t) { topWeek = old }
        return true
    }

    /// Move the selected day, keeping it on screen.
    private func shiftDay(_ by: Int) {
        let cal = Calendar.current
        guard let d = cal.date(byAdding: .day, value: by, to: selected) else { return }
        withAnimation(.snappy) {
            selected = d
            let top = topWeek ?? GoogleStore.startOfWeek(d)
            let visibleEnd = cal.date(byAdding: .day, value: 7 * rows, to: top) ?? top
            if d < top || d >= visibleEnd {
                moving = true
                topWeek = GoogleStore.startOfWeek(mode == "month" ? Self.startOfMonth(d) : d)
                Task {
                    try? await Task.sleep(for: .milliseconds(700))
                    moving = false
                }
            }
        }
    }

    /// The month the visible rows mostly show.
    private var focusMonth: Date {
        let cal = Calendar.current
        let top = topWeek ?? GoogleStore.startOfWeek(selected)
        if mode != "month" { return Self.startOfMonth(cal.date(byAdding: .day, value: 3, to: top) ?? top) }
        return Self.startOfMonth(cal.date(byAdding: .day, value: 17, to: top) ?? top)
    }

    // MARK: calendar card

    private var calendarCard: some View {
        let cal = Calendar.current
        let head = weeks.first ?? .now
        return VStack(spacing: 6) {
            HStack(spacing: 0) {
                ForEach(0..<7, id: \.self) { i in
                    let d = cal.date(byAdding: .day, value: i, to: head) ?? head
                    Text(d.formatted(.dateTime.weekday(.narrow)))
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(Palette.weekday(d) ?? .secondary)
                        .frame(maxWidth: .infinity)
                }
            }
            Group {
                if mode == "month" {
                    // Scrolls vertically inside the card, snapping to whole weeks.
                    ScrollView(.vertical, showsIndicators: false) {
                        LazyVStack(spacing: 0) { weekRows }.scrollTargetLayout()
                    }
                    .scrollTargetBehavior(.viewAligned)
                } else {
                    // One week per page; swipe sideways.
                    ScrollView(.horizontal, showsIndicators: false) {
                        LazyHStack(spacing: 0) { weekRows }.scrollTargetLayout()
                    }
                    .scrollTargetBehavior(.paging)
                }
            }
            .scrollPosition(id: $topWeek, anchor: mode == "month" ? .top : .leading)
            .frame(height: Self.rowHeight * CGFloat(rows))
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .onChange(of: topWeek) { old, new in
                if guardJump(from: old ?? lastTop, to: new) { return }
                if let new { lastTop = new }
                // In week view the selection follows the week (same weekday).
                guard mode == "week", let old, let new, old != new else { return }
                let offset = cal.dateComponents([.day], from: old, to: selected).day ?? 0
                if (0..<7).contains(offset) { selected = cal.date(byAdding: .day, value: offset, to: new) ?? new }
            }
        }
        .card(padding: 12)
        .padding(.top, 8)
        .sensoryFeedback(.selection, trigger: selected)
    }

    @ViewBuilder private var weekRows: some View {
        let month = mode == "month" ? focusMonth : nil
        ForEach(weeks, id: \.self) { w in
            WeekRow(days: days(of: w, month: month)) { d in
                withAnimation(.snappy) { selected = d }
            }
            .equatable()
            .frame(height: Self.rowHeight)
            .containerRelativeFrame(.horizontal, alignment: .center) { width, _ in width }
            .id(w)
        }
    }

    /// Plain values per day, so a row only redraws when one of them changes.
    private func days(of w: Date, month: Date?) -> [DayCell] {
        let cal = Calendar.current
        return (0..<7).map { i in
            let d = cal.date(byAdding: .day, value: i, to: w) ?? w
            return DayCell(
                date: d,
                selected: cal.isDate(d, inSameDayAs: selected),
                today: cal.isDateInToday(d),
                faded: month.map { !cal.isDate(d, equalTo: $0, toGranularity: .month) } ?? false,
                dots: google.events(on: d).prefix(3).map { Palette.event(($0.id, $0.colorId)) })
        }
    }

    // MARK: day

    private var dayHeader: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(Calendar.current.isDateInToday(selected) ? String(localized: "Today") : selected.formatted(.dateTime.weekday(.wide)))
                .font(.system(.title2, design: .rounded).weight(.bold))
            Text(selected.formatted(.dateTime.month(.abbreviated).day())).foregroundStyle(.secondary)
            Spacer()
        }
        // An overlay, so the spinner never changes the header's height.
        .overlay(alignment: .trailing) {
            if google.loading.contains("events") && !google.loadedWeeks.contains(Self.startOfWeek(selected)) {
                ProgressView().controlSize(.small)
            }
        }
    }

    @ViewBuilder private var dayEvents: some View {
        let list = google.events(on: selected)
        Group {
            if list.isEmpty && google.loadedWeeks.contains(Self.startOfWeek(selected)) {
                SoftEmpty(title: "Nothing planned", systemImage: "sun.haze", message: "A free day ☕️")
                    .card()
            } else {
                VStack(spacing: 8) {
                    ForEach(list) { e in
                        EventRow(event: e)
                            .onTapGesture { detail = e }
                            .contextMenu {
                                Button("Delete", systemImage: "trash", role: .destructive) { confirmDelete = e }
                            }
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, minHeight: 120, alignment: .top)
        .contentShape(Rectangle())
        // Swipe sideways over the day to go to the previous or next day.
        .simultaneousGesture(
            DragGesture(minimumDistance: 24).onEnded { v in
                let dx = v.translation.width, dy = v.translation.height
                guard abs(dx) > 60, abs(dx) > abs(dy) * 1.5 else { return }
                shiftDay(dx < 0 ? 1 : -1)
            })
    }

    // MARK: data

    /// Visible rows plus a week either side.
    private func load(force: Bool) async {
        let cal = Calendar.current
        let top = topWeek ?? GoogleStore.startOfWeek(selected)
        guard let from = cal.date(byAdding: .day, value: -7, to: top), let to = cal.date(byAdding: .day, value: 7 * (rows + 1), to: top) else { return }
        await google.loadRange(from: from, to: to, force: force)
    }

    static func startOfWeek(_ d: Date) -> Date { GoogleStore.startOfWeek(d) }

    static func startOfMonth(_ d: Date) -> Date {
        let cal = Calendar.current
        return cal.dateInterval(of: .month, for: d)?.start ?? cal.startOfDay(for: d)
    }
}

struct DayCell: Equatable {
    let date: Date
    let selected: Bool
    let today: Bool
    let faded: Bool
    let dots: [Color]
}

/// One week of the calendar grid.
struct WeekRow: View, @MainActor Equatable {
    let days: [DayCell]
    var select: (Date) -> Void

    static func == (a: WeekRow, b: WeekRow) -> Bool { a.days == b.days }

    var body: some View {
        HStack(spacing: 4) {
            ForEach(days, id: \.date) { cell($0) }
        }
        .padding(.vertical, 3)
        .padding(.horizontal, 1)
    }

    private func cell(_ c: DayCell) -> some View {
        Button { select(c.date) } label: {
            VStack(spacing: 3) {
                Text(c.date.formatted(.dateTime.day()))
                    .font(.system(.subheadline, design: .rounded).weight(c.today || c.selected ? .bold : .medium))
                HStack(spacing: 2) {
                    ForEach(c.dots.indices, id: \.self) { i in
                        Circle().fill(c.selected ? Color.white : c.dots[i]).frame(width: 4, height: 4)
                    }
                }
                .frame(height: 4)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .foregroundStyle(c.selected ? .white : (c.today ? Palette.lavender : (Palette.weekday(c.date) ?? .primary)))
            .opacity(c.faded && !c.selected ? 0.35 : 1)
            .background {
                if c.selected {
                    RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Palette.accent)
                } else if c.today {
                    RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Palette.lavender.opacity(0.6), lineWidth: 1.5)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// One event as a compact row: time on the left, title and place on the right.
struct EventRow: View {
    let event: CalendarEvent

    var body: some View {
        let color = Palette.event((event.id, event.colorId))
        HStack(spacing: 10) {
            Capsule().fill(color).frame(width: 4)
            Group {
                if event.allDay {
                    Text("All day").font(.caption.weight(.semibold)).foregroundStyle(color)
                } else if let s = event.startDate {
                    VStack(alignment: .leading, spacing: 0) {
                        Text(Fmt.time(s)).font(.subheadline.weight(.semibold).monospacedDigit())
                        if let e = event.endDate { Text(Fmt.time(e)).font(.caption2.monospacedDigit()).foregroundStyle(.secondary) }
                    }
                }
            }
            .frame(width: 48, alignment: .leading)
            VStack(alignment: .leading, spacing: 1) {
                Text(event.displayTitle).font(.subheadline.weight(.semibold)).lineLimit(1)
                if let loc = event.location, !loc.isEmpty {
                    Text(loc).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            Spacer(minLength: 0)
        }
        // Every row is the same size, with or without an end time or place.
        .frame(maxWidth: .infinity, minHeight: 38, maxHeight: 38, alignment: .leading)
        .padding(.vertical, 8)
        .padding(.horizontal, 10)
        .softSurface(radius: 14)
        .contentShape(Rectangle())
    }
}

struct EventDetailSheet: View {
    @Bindable var google: GoogleStore
    /// As opened; the live copy (with a changed color) comes from the store.
    let opened: CalendarEvent
    @Environment(\.openURL) private var openURL

    init(google: GoogleStore, event: CalendarEvent) {
        self.google = google
        opened = event
    }

    private var event: CalendarEvent { google.eventsById[opened.id] ?? opened }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Text(event.displayTitle).font(.system(.title, design: .rounded).weight(.bold))
                    VStack(alignment: .leading, spacing: 10) {
                        if let s = event.startDate {
                            Label(event.allDay ? s.formatted(date: .complete, time: .omitted) : "\(s.formatted(date: .abbreviated, time: .shortened)) – \(event.endDate.map(Fmt.time) ?? "")", systemImage: "calendar")
                        }
                        if let l = event.location, !l.isEmpty { Label(l, systemImage: "mappin.and.ellipse") }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .card()
                    ColorTagPicker(selection: Binding(get: { event.colorId }, set: { c in Task { await google.setColor(event, c) } }), fallback: Palette.event((event.id, nil)))
                        .card(padding: 12)
                    if let d = event.description, !d.isEmpty {
                        Text(d).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading).card()
                    }
                    if let link = event.link.flatMap(URL.init(string:)) {
                        Button { openURL(link) } label: { Label("Open in Google Calendar", systemImage: "arrow.up.right.square") }
                            .tint(Palette.lavender)
                    }
                }
                .padding()
            }
            .background(PastelBackground())
        }
        .presentationDetents([.medium, .large])
    }
}

struct NewEventSheet: View {
    @Bindable var google: GoogleStore
    @Environment(\.dismiss) private var dismiss
    @State private var title = ""
    @State private var allDay = false
    @State private var start: Date
    @State private var end: Date
    @State private var notes = ""
    @State private var colorId: String?

    init(google: GoogleStore, day: Date) {
        self.google = google
        let cal = Calendar.current
        // Next full hour on the chosen day.
        let hour = cal.component(.hour, from: .now) + 1
        let s = cal.date(bySettingHour: min(hour, 23), minute: 0, second: 0, of: day) ?? day
        _start = State(initialValue: s)
        _end = State(initialValue: s.addingTimeInterval(3600))
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Title", text: $title).font(.title3.weight(.semibold))
                }
                Section {
                    Toggle("All day", isOn: $allDay.animation()).tint(Palette.pink)
                    DatePicker("Starts", selection: $start, displayedComponents: allDay ? .date : [.date, .hourAndMinute])
                    DatePicker("Ends", selection: $end, in: start..., displayedComponents: allDay ? .date : [.date, .hourAndMinute])
                }
                Section("Color") {
                    ColorTagPicker(selection: $colorId, fallback: Palette.lavender)
                        .listRowInsets(EdgeInsets(top: 10, leading: 12, bottom: 10, trailing: 12))
                }
                Section("Notes") {
                    TextField("Notes", text: $notes, axis: .vertical).lineLimit(3...8)
                }
            }
            .scrollContentBackground(.hidden)
            .background(PastelBackground())
            .onChange(of: start) { old, new in
                // Keep the duration when the start moves.
                end = new.addingTimeInterval(max(end.timeIntervalSince(old), allDay ? 0 : 900))
            }
            .navigationTitle("New event")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    if google.loading.contains("save") {
                        ProgressView()
                    } else {
                        Button("Add") {
                            Task {
                                if await google.addEvent(summary: title, start: start, end: end, allDay: allDay, location: "", notes: notes, colorId: colorId) { dismiss() }
                            }
                        }
                        .bold()
                        .disabled(title.trimmingCharacters(in: .whitespaces).isEmpty)
                    }
                }
            }
            .googleErrors(google)
        }
    }
}

/// Google Calendar's color tags as a row of dots; the first dot is "no tag"
/// (the calendar's own color).
struct ColorTagPicker: View {
    @Binding var selection: String?
    /// Shown for "no tag".
    let fallback: Color

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 10) {
                dot(nil, color: fallback, name: "Default")
                ForEach(EventColor.tags) { t in dot(t.id, color: Palette.tag(t), name: t.name) }
            }
            .padding(.vertical, 4)
            .padding(.horizontal, 2)
        }
        .sensoryFeedback(.selection, trigger: selection)
    }

    private func dot(_ id: String?, color: Color, name: String) -> some View {
        let on = selection == id
        return Button { withAnimation(.snappy) { selection = id } } label: {
            ZStack {
                Circle().fill(color).frame(width: 28, height: 28)
                if id == nil {
                    // "No tag": a soft ring instead of a solid dot.
                    Circle().fill(.background).frame(width: 14, height: 14)
                }
                if on {
                    Circle().strokeBorder(.white, lineWidth: 2.5).frame(width: 28, height: 28)
                    Circle().strokeBorder(color, lineWidth: 2).frame(width: 36, height: 36)
                }
            }
            .frame(width: 38, height: 38)
            .scaleEffect(on ? 1.05 : 1)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(name)
        .accessibilityAddTraits(on ? .isSelected : [])
    }
}
