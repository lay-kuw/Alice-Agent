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

    /// About three years either way; rows are created lazily.
    private let weeks: [Date] = {
        let cal = Calendar.current
        let now = GoogleStore.startOfWeek(.now)
        return (-156...156).compactMap { cal.date(byAdding: .day, value: 7 * $0, to: now) }
    }()

    private var rows: Int { mode == "month" ? 6 : 1 }
    private static let rowHeight: CGFloat = 50

    var body: some View {
        NavigationStack {
            GoogleGate(google: google, group: "calendar", title: "Calendar") {
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        calendarCard
                        dayHeader
                        dayEvents
                    }
                    .padding(.horizontal)
                    .padding(.bottom, 30)
                }
                .refreshable { await load(force: true) }
                .background(PastelBackground())
                .task(id: "\(mode)|\(topWeek?.timeIntervalSince1970 ?? 0)") {
                    // Wait for scrolling to settle before loading.
                    try? await Task.sleep(for: .milliseconds(150))
                    await load(force: false)
                }
            }
            .navigationTitle(focusMonth.formatted(.dateTime.year().month(.wide)))
            .onAppear {
                #if DEBUG
                if let m = ProcessInfo.processInfo.environment["ALICE_DEV_CALENDAR"] { mode = m }
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
            .sheet(item: $detail) { EventDetailSheet(event: $0) }
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
                Label("Week", systemImage: "rectangle.split.1x2").tag("week")
                Label("Month", systemImage: "calendar").tag("month")
            }
            Divider()
            Button("Jump to today", systemImage: "sun.max") { jumpToToday() }
        } label: {
            HStack(spacing: 4) {
                Text(mode == "month" ? "Month" : "Week")
                Image(systemName: "chevron.down").font(.caption2.bold())
            }
            .font(.subheadline.weight(.semibold))
            .foregroundStyle(Palette.lavender)
        }
    }

    private func setMode(_ m: String) {
        withAnimation(.snappy) {
            mode = m
            let anchor = m == "month" ? Self.startOfMonth(selected) : selected
            topWeek = GoogleStore.startOfWeek(anchor)
        }
    }

    private func jumpToToday() {
        withAnimation(.snappy) {
            selected = Calendar.current.startOfDay(for: .now)
            topWeek = GoogleStore.startOfWeek(mode == "month" ? Self.startOfMonth(.now) : .now)
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
                    Text((cal.date(byAdding: .day, value: i, to: head) ?? head).formatted(.dateTime.weekday(.narrow)))
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity)
                }
            }
            // Scrolls vertically inside the card, snapping to whole weeks.
            ScrollView(.vertical, showsIndicators: false) {
                LazyVStack(spacing: 0) {
                    ForEach(weeks, id: \.self) { w in
                        weekRow(w).frame(height: Self.rowHeight).id(w)
                    }
                }
                .scrollTargetLayout()
            }
            .scrollTargetBehavior(.viewAligned)
            .scrollPosition(id: $topWeek, anchor: .top)
            .frame(height: Self.rowHeight * CGFloat(rows))
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .onChange(of: topWeek) { old, new in
                // In week view the selection follows the week (same weekday).
                guard mode == "week", let old, let new, old != new else { return }
                let offset = cal.dateComponents([.day], from: old, to: selected).day ?? 0
                if (0..<7).contains(offset) { selected = cal.date(byAdding: .day, value: offset, to: new) ?? new }
            }
        }
        .card(padding: 12)
        .padding(.top, 8)
        .animation(.snappy, value: mode)
    }

    private func weekRow(_ w: Date) -> some View {
        let cal = Calendar.current
        return HStack(spacing: 4) {
            ForEach(0..<7, id: \.self) { i in
                dayCell(cal.date(byAdding: .day, value: i, to: w) ?? w)
            }
        }
        .padding(.vertical, 3)
    }

    private func dayCell(_ d: Date) -> some View {
        let cal = Calendar.current
        let isSel = cal.isDate(d, inSameDayAs: selected)
        let isToday = cal.isDateInToday(d)
        // Month view fades days outside the month in focus.
        let faded = mode == "month" && !cal.isDate(d, equalTo: focusMonth, toGranularity: .month)
        let events = google.events(on: d)
        return Button {
            withAnimation(.snappy) { selected = d }
        } label: {
            VStack(spacing: 3) {
                Text(d.formatted(.dateTime.day()))
                    .font(.system(.subheadline, design: .rounded).weight(isToday || isSel ? .bold : .medium))
                HStack(spacing: 2) {
                    ForEach(events.prefix(3)) { e in
                        Circle().fill(isSel ? Color.white : Palette.event((e.id, e.colorId))).frame(width: 4, height: 4)
                    }
                }
                .frame(height: 4)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .foregroundStyle(isSel ? .white : (isToday ? Palette.lavender : .primary))
            .opacity(faded && !isSel ? 0.35 : 1)
            .background {
                if isSel {
                    RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Palette.accent).shadow(color: Palette.pink.opacity(0.35), radius: 5, y: 2)
                } else if isToday {
                    RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Palette.lavender.opacity(0.6), lineWidth: 1.5)
                }
            }
        }
        .buttonStyle(.plain)
        .sensoryFeedback(.selection, trigger: isSel)
    }

    // MARK: day

    private var dayHeader: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(Calendar.current.isDateInToday(selected) ? String(localized: "Today") : selected.formatted(.dateTime.weekday(.wide)))
                .font(.system(.title2, design: .rounded).weight(.bold))
            Text(selected.formatted(.dateTime.month(.abbreviated).day())).foregroundStyle(.secondary)
            Spacer()
            if google.loading.contains("events") { ProgressView().controlSize(.small).opacity(google.loadedWeeks.contains(Self.startOfWeek(selected)) ? 0.4 : 1) }
        }
    }

    @ViewBuilder private var dayEvents: some View {
        let list = google.events(on: selected)
        if list.isEmpty && google.loadedWeeks.contains(Self.startOfWeek(selected)) {
            SoftEmpty(title: "Nothing planned", systemImage: "sun.haze", message: "A free day ☕️")
                .card()
        } else {
            VStack(spacing: 10) {
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

struct EventRow: View {
    let event: CalendarEvent

    var body: some View {
        let color = Palette.event((event.id, event.colorId))
        HStack(spacing: 12) {
            Capsule().fill(LinearGradient(colors: [color, color.opacity(0.6)], startPoint: .top, endPoint: .bottom)).frame(width: 5)
            VStack(alignment: .leading, spacing: 4) {
                Text(event.displayTitle).font(.body.weight(.semibold)).lineLimit(2)
                HStack(spacing: 6) {
                    if event.allDay {
                        Chip(text: String(localized: "All day"), systemImage: "sun.max", color: color)
                    } else if let s = event.startDate {
                        Image(systemName: "clock").font(.caption)
                        Text("\(Fmt.time(s)) – \(event.endDate.map(Fmt.time) ?? "")").font(.subheadline)
                    }
                }
                .foregroundStyle(.secondary)
                if let loc = event.location, !loc.isEmpty {
                    Label(loc, systemImage: "mappin.and.ellipse").font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .card(padding: 12)
    }
}

struct EventDetailSheet: View {
    let event: CalendarEvent
    @Environment(\.openURL) private var openURL

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
    @State private var location = ""
    @State private var notes = ""

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
                    TextField("Location", text: $location)
                }
                Section {
                    Toggle("All day", isOn: $allDay.animation()).tint(Palette.pink)
                    DatePicker("Starts", selection: $start, displayedComponents: allDay ? .date : [.date, .hourAndMinute])
                    DatePicker("Ends", selection: $end, in: start..., displayedComponents: allDay ? .date : [.date, .hourAndMinute])
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
                                if await google.addEvent(summary: title, start: start, end: end, allDay: allDay, location: location, notes: notes) { dismiss() }
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
