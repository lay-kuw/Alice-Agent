import AliceKit
import SwiftUI

struct CalendarView: View {
    @Bindable var google: GoogleStore
    @State private var selected = Calendar.current.startOfDay(for: .now)
    @State private var weekStart = CalendarView.startOfWeek(.now)
    @State private var month = CalendarView.startOfMonth(.now)
    @AppStorage("calendar.mode") private var mode = "week"
    @State private var adding = false
    @State private var detail: CalendarEvent?
    @State private var confirmDelete: CalendarEvent?

    var body: some View {
        NavigationStack {
            GoogleGate(google: google, group: "calendar", title: "Calendar") {
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        Picker("View", selection: $mode.animation(.snappy)) {
                            Text("Week").tag("week")
                            Text("Month").tag("month")
                        }
                        .pickerStyle(.segmented)
                        .padding(.top, 4)
                        if mode == "month" { monthGrid } else { weekStrip }
                        dayHeader
                        dayEvents
                    }
                    .padding(.horizontal)
                    .padding(.bottom, 30)
                }
                .refreshable { await load() }
                .background(PastelBackground())
                .task(id: "\(mode)|\(weekStart)|\(month)") { await load() }
            }
            .navigationTitle((mode == "month" ? month : weekStart).formatted(.dateTime.year().month(.wide)))
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Today") {
                        withAnimation(.snappy) {
                            selected = Calendar.current.startOfDay(for: .now)
                            weekStart = Self.startOfWeek(.now)
                            month = Self.startOfMonth(.now)
                        }
                    }
                }
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

    // MARK: month grid

    private var monthGrid: some View {
        let cal = Calendar.current
        let start = Self.startOfWeek(month)
        let days = (0..<42).compactMap { cal.date(byAdding: .day, value: $0, to: start) }
        let symbols = (0..<7).compactMap { cal.date(byAdding: .day, value: $0, to: start) }
        return VStack(spacing: 8) {
            HStack {
                Button { shiftMonth(-1) } label: { Image(systemName: "chevron.left") }
                Spacer()
                Text(month.formatted(.dateTime.month(.wide))).font(.subheadline.weight(.semibold)).foregroundStyle(.secondary)
                Spacer()
                Button { shiftMonth(1) } label: { Image(systemName: "chevron.right") }
            }
            .tint(Palette.lavender)
            HStack(spacing: 0) {
                ForEach(symbols, id: \.self) { d in
                    Text(d.formatted(.dateTime.weekday(.narrow))).font(.caption2.weight(.semibold)).foregroundStyle(.secondary).frame(maxWidth: .infinity)
                }
            }
            LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 4), count: 7), spacing: 4) {
                ForEach(days, id: \.self) { d in monthCell(d) }
            }
        }
        .card(padding: 12)
        .padding(.top, 4)
        .gesture(DragGesture(minimumDistance: 30).onEnded { v in
            if v.translation.width < -40 { shiftMonth(1) } else if v.translation.width > 40 { shiftMonth(-1) }
        })
    }

    private func monthCell(_ d: Date) -> some View {
        let cal = Calendar.current
        let inMonth = cal.isDate(d, equalTo: month, toGranularity: .month)
        let isSel = cal.isDate(d, inSameDayAs: selected)
        let isToday = cal.isDateInToday(d)
        let events = google.events(on: d)
        return Button {
            withAnimation(.snappy) {
                selected = d
                weekStart = Self.startOfWeek(d)
                if !inMonth { month = Self.startOfMonth(d) }
            }
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
            .frame(maxWidth: .infinity, minHeight: 40)
            .foregroundStyle(isSel ? .white : (isToday ? Palette.lavender : (inMonth ? .primary : .secondary)))
            .opacity(inMonth || isSel ? 1 : 0.45)
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

    private func shiftMonth(_ n: Int) {
        let cal = Calendar.current
        guard let m = cal.date(byAdding: .month, value: n, to: month) else { return }
        withAnimation(.snappy) {
            month = m
            // Keep the same day of month where possible.
            let day = min(cal.component(.day, from: selected), cal.range(of: .day, in: .month, for: m)?.count ?? 28)
            selected = cal.date(bySetting: .day, value: day, of: m) ?? m
            weekStart = Self.startOfWeek(selected)
        }
    }

    // MARK: week strip

    private var weekStrip: some View {
        VStack(spacing: 10) {
            HStack {
                Button { shiftWeek(-1) } label: { Image(systemName: "chevron.left") }
                Spacer()
                Text(weekLabel).font(.subheadline.weight(.semibold)).foregroundStyle(.secondary)
                Spacer()
                Button { shiftWeek(1) } label: { Image(systemName: "chevron.right") }
            }
            .tint(Palette.lavender)
            HStack(spacing: 6) {
                ForEach(days, id: \.self) { d in dayPill(d) }
            }
        }
        .card(padding: 12)
        .padding(.top, 8)
        .gesture(DragGesture(minimumDistance: 30).onEnded { v in
            if v.translation.width < -40 { shiftWeek(1) } else if v.translation.width > 40 { shiftWeek(-1) }
        })
    }

    private func dayPill(_ d: Date) -> some View {
        let cal = Calendar.current
        let isSel = cal.isDate(d, inSameDayAs: selected)
        let isToday = cal.isDateInToday(d)
        let count = google.events(on: d).count
        return Button {
            withAnimation(.snappy) {
                selected = d
                month = Self.startOfMonth(d)
            }
        } label: {
            VStack(spacing: 4) {
                Text(d.formatted(.dateTime.weekday(.narrow))).font(.caption2.weight(.semibold)).opacity(0.7)
                Text(d.formatted(.dateTime.day())).font(.system(.headline, design: .rounded).weight(isToday ? .heavy : .semibold))
                HStack(spacing: 2) {
                    ForEach(0..<min(count, 3), id: \.self) { _ in Circle().frame(width: 4, height: 4) }
                }
                .frame(height: 4)
                .foregroundStyle(isSel ? .white : Palette.pink)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 8)
            .foregroundStyle(isSel ? .white : (isToday ? Palette.lavender : .primary))
            .background {
                if isSel {
                    RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Palette.accent).shadow(color: Palette.pink.opacity(0.4), radius: 6, y: 3)
                } else if isToday {
                    RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Palette.lavender.opacity(0.6), lineWidth: 1.5)
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

    private var days: [Date] {
        (0..<7).compactMap { Calendar.current.date(byAdding: .day, value: $0, to: weekStart) }
    }

    private var weekLabel: String {
        guard let last = days.last else { return "" }
        return "\(weekStart.formatted(.dateTime.month(.abbreviated).day())) – \(last.formatted(.dateTime.month(.abbreviated).day()))"
    }

    private func shiftWeek(_ n: Int) {
        let cal = Calendar.current
        guard let w = cal.date(byAdding: .day, value: 7 * n, to: weekStart) else { return }
        withAnimation(.snappy) {
            weekStart = w
            selected = cal.date(byAdding: .day, value: 7 * n, to: selected) ?? w
            month = Self.startOfMonth(selected)
        }
    }

    private func load() async {
        if mode == "month" { await google.loadMonth(month) } else { await google.loadWeek(weekStart) }
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
