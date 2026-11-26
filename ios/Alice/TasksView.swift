import AliceKit
import SwiftUI

struct TasksView: View {
    @Bindable var google: GoogleStore
    /// The list shown, or nil for every list grouped by list.
    @AppStorage("tasks.list") private var listId = ""
    @State private var showCompleted = false
    /// all / today / week
    @AppStorage("tasks.view") private var scope = "all"
    @State private var openGroups: Set<String> = []
    @State private var adding = false
    @State private var editing: Editing?

    struct Editing: Identifiable {
        let task: TaskItem
        let list: String
        var id: String { task.id }
    }

    var body: some View {
        NavigationStack {
            GoogleGate(google: google, group: "tasks", title: "Tasks") {
                List {
                    BlurbBubble(screen: "tasks", fallback: "Let's take it one at a time ✅")
                        .listRowBackground(Color.clear)
                        .listRowSeparator(.hidden)
                        .listRowInsets(EdgeInsets(top: 4, leading: 16, bottom: 4, trailing: 16))
                    scopeChips
                        .listRowBackground(Color.clear)
                        .listRowSeparator(.hidden)
                    if scope == "today" {
                        todaySection
                    } else if scope == "week" {
                        weekSection
                    } else {
                        allSection
                    }
                }
                .listStyle(.plain)
                .animation(.snappy, value: google.lists)
                .pastel()
                .overlay { if !google.tasksLoaded { ProgressView() } }
                .refreshable { await google.loadTasks() }
                .task { if !google.tasksLoaded { await google.loadTasks() } }
                .task(id: "\(BlurbFacts.tasks(google))\(Blurbs.shared.enabled)") { await Blurbs.shared.ask("tasks", facts: BlurbFacts.tasks(google), client: google.client) }
            }
            .navigationTitle("To-do")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button { adding = true } label: { Image(systemName: "plus.circle.fill").font(.title3).foregroundStyle(Palette.accent) }
                        .accessibilityLabel("New task")
                        .disabled(google.lists.isEmpty)
                }
            }
            #if DEBUG
            .task(id: google.tasksLoaded) {
                if google.tasksLoaded, ProcessInfo.processInfo.environment["ALICE_DEV_ADD"] != nil { adding = true }
            }
            #endif
            .sheet(isPresented: $adding) {
                // New tasks go to the list in view (the first list in the All view).
                TaskSheet(google: google, listId: selected?.id ?? google.lists.first?.id ?? "")
            }
            .sheet(item: $editing) { e in TaskSheet(google: google, listId: e.list, editing: e.task) }
            .googleErrors(google)
        }
    }

    // MARK: scopes

    private var scopeChips: some View {
        HStack(spacing: 8) {
            scopeChip("all", "scope.all", "tray.full")
            scopeChip("today", "scope.today", "sun.max")
            scopeChip("week", "scope.week", "calendar")
        }
    }

    private func scopeChip(_ id: String, _ title: LocalizedStringKey, _ icon: String) -> some View {
        Button { withAnimation(.snappy) { scope = id } } label: {
            Label(title, systemImage: icon)
                .font(.subheadline.weight(.semibold))
                .lineLimit(1)
                .minimumScaleFactor(0.8)
                .frame(maxWidth: .infinity)
                .padding(.vertical, 8)
                .foregroundStyle(scope == id ? .white : .primary)
                .background {
                    if scope == id { Capsule().fill(Palette.accent) } else { Capsule().fill(.background.opacity(0.6)) }
                }
        }
        .buttonStyle(.plain)
    }

    /// Every task with its list, for the date views.
    private var allTasks: [(String, TaskItem)] { google.lists.flatMap { l in l.tasks.map { (l.id, $0) } } }

    private func dayOffset(_ t: TaskItem) -> Int? {
        guard let d = t.dueDate else { return nil }
        let cal = Calendar.current
        return cal.dateComponents([.day], from: cal.startOfDay(for: .now), to: cal.startOfDay(for: d)).day
    }

    /// Due today or overdue (and today's that are done), with progress, then tomorrow and the rest of the week.
    @ViewBuilder private var todaySection: some View {
        let today = allTasks.filter { _, t in
            guard let o = dayOffset(t) else { return false }
            return t.completed ? o == 0 : o <= 0
        }
        let done = today.filter(\.1.completed).count
        progress("Today's tasks", done: done, total: today.count)
        if today.isEmpty && google.tasksLoaded {
            Text("Nothing due today 🌷").foregroundStyle(.secondary).listRowBackground(Color.clear).listRowSeparator(.hidden)
        }
        ForEach(today, id: \.1.id) { list, t in row(t, list: list) }
        group("tomorrow", "Tomorrow", allTasks.filter { !$0.1.completed && dayOffset($0.1) == 1 })
        group("week", "This week", allTasks.filter { !$0.1.completed && (dayOffset($0.1).map { (2...7).contains($0) } ?? false) })
    }

    /// Tasks due within a week (open ones, overdue too, and those done), with progress.
    @ViewBuilder private var weekSection: some View {
        let week = allTasks.filter { _, t in dayOffset(t).map { t.completed ? (0...7).contains($0) : $0 <= 7 } ?? false }
            .sorted { ($0.1.completed ? 1 : 0, $0.1.dueDate ?? .distantFuture) < ($1.1.completed ? 1 : 0, $1.1.dueDate ?? .distantFuture) }
        let soon = week
        let done = week.filter(\.1.completed).count
        progress("This week's tasks", done: done, total: week.count)
        if soon.isEmpty && google.tasksLoaded {
            SoftEmpty(title: "A light week", systemImage: "leaf", message: "Nothing due in the next seven days")
                .listRowBackground(Color.clear).listRowSeparator(.hidden)
        }
        ForEach(soon, id: \.1.id) { list, t in row(t, list: list) }
    }

    /// "Today's tasks 2/4" with a soft bar.
    private func progress(_ title: LocalizedStringKey, done: Int, total: Int) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline) {
                Text(title).font(.headline.weight(.bold))
                Text("\(done)/\(total)").font(.subheadline.weight(.semibold)).foregroundStyle(.secondary)
                Spacer()
                Image(systemName: "sparkles").foregroundStyle(Palette.accent)
            }
            ProgressView(value: total == 0 ? 0 : Double(done) / Double(total)).tint(Palette.lavender)
        }
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    /// A folding group with a count, like "Tomorrow 3 ⌄".
    @ViewBuilder private func group(_ id: String, _ title: LocalizedStringKey, _ items: [(String, TaskItem)]) -> some View {
        if !items.isEmpty {
            Button {
                withAnimation(.snappy) { if openGroups.contains(id) { openGroups.remove(id) } else { openGroups.insert(id) } }
            } label: {
                HStack {
                    Image(systemName: "calendar").foregroundStyle(Palette.accent)
                    Text(title).font(.subheadline.weight(.bold))
                    Spacer()
                    Text("\(items.count)").font(.caption.weight(.bold)).foregroundStyle(Palette.lavender)
                        .padding(.horizontal, 8).padding(.vertical, 2).background(Palette.lavender.opacity(0.15), in: Capsule())
                    Image(systemName: "chevron.down").font(.caption.bold()).rotationEffect(.degrees(openGroups.contains(id) ? 180 : 0))
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 12)
                .glassCard(radius: 16)
            }
            .buttonStyle(.plain)
            .listRowBackground(Color.clear)
            .listRowSeparator(.hidden)
            if openGroups.contains(id) {
                ForEach(items, id: \.1.id) { list, t in row(t, list: list) }
            }
        }
    }

    /// Every list (or one), as before.
    @ViewBuilder private var allSection: some View {
                    if !google.lists.isEmpty { listChips.listRowBackground(Color.clear).listRowSeparator(.hidden) }
                    let lists = shown
                    let open = lists.flatMap { l in l.tasks.filter { !$0.completed }.map { (l, $0) } }
                    let done = lists.flatMap { l in l.tasks.filter(\.completed).map { (l, $0) } }
                    if open.isEmpty && google.tasksLoaded {
                        SoftEmpty(title: "All done!", systemImage: "checkmark.seal", message: "Enjoy your free time 🌷")
                            .listRowBackground(Color.clear).listRowSeparator(.hidden)
                    }
                    ForEach(lists) { l in
                        let items = l.tasks.filter { !$0.completed }
                        if lists.count > 1 && !items.isEmpty {
                            // Group header in the All view.
                            Text(l.title)
                                .font(.footnote.weight(.bold))
                                .foregroundStyle(Palette.lavender)
                                .listRowInsets(EdgeInsets(top: 10, leading: 20, bottom: 0, trailing: 16))
                                .listRowBackground(Color.clear)
                                .listRowSeparator(.hidden)
                        }
                        ForEach(items) { t in row(t, list: l.id) }
                    }
                    if !done.isEmpty {
                        Button {
                            withAnimation(.snappy) { showCompleted.toggle() }
                        } label: {
                            HStack {
                                Text("Completed (\(done.count))").font(.subheadline.weight(.semibold))
                                Image(systemName: "chevron.right").font(.caption.bold()).rotationEffect(.degrees(showCompleted ? 90 : 0))
                            }
                            .foregroundStyle(.secondary)
                        }
                        .buttonStyle(.plain)
                        .listRowBackground(Color.clear)
                        .listRowSeparator(.hidden)
                        .padding(.top, 8)
                        if showCompleted { ForEach(done, id: \.1.id) { l, t in row(t, list: l.id) } }
                    }
    }

    /// The chosen list; nil means All.
    private var selected: TaskList? { google.lists.first { $0.id == listId } }

    private var shown: [TaskList] { selected.map { [$0] } ?? google.lists }

    private var listChips: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                chip(L("All"), id: "", count: google.lists.reduce(0) { $0 + $1.tasks.filter { !$0.completed }.count })
                ForEach(google.lists) { l in
                    chip(l.title, id: l.id, count: l.tasks.filter { !$0.completed }.count)
                }
            }
        }
    }

    private func chip(_ title: String, id: String, count: Int) -> some View {
        let sel = (selected?.id ?? "") == id
        return Button {
            withAnimation(.snappy) { listId = id }
        } label: {
            HStack(spacing: 5) {
                Text(title)
                if count > 0 { Text("\(count)").font(.caption.weight(.bold)).opacity(0.7) }
            }
            .font(.subheadline.weight(.semibold))
            .padding(.horizontal, 14)
            .padding(.vertical, 7)
            .foregroundStyle(sel ? .white : .primary)
            .background {
                if sel { Capsule().fill(Palette.accent) } else { Capsule().fill(.ultraThinMaterial) }
            }
        }
        .buttonStyle(.plain)
    }

    private func row(_ t: TaskItem, list: String) -> some View {
        TaskRow(task: t) { Task { await google.toggle(t, in: list) } }
            .padding(.leading, t.parent == nil ? 0 : 28)
            // Press and hold: the row pops up with Edit and Delete.
            .contextMenu {
                if !t.id.hasPrefix("new-") {
                    Button("Edit", systemImage: "pencil") { editing = Editing(task: t, list: list) }
                    Button("Delete", systemImage: "trash", role: .destructive) { Task { await google.deleteTask(t, in: list) } }
                }
            }
            .listRowBackground(Color.clear)
            .listRowSeparatorTint(Palette.lavender.opacity(0.2))
            .swipeActions {
                Button(role: .destructive) { Task { await google.deleteTask(t, in: list) } } label: { Label("Delete", systemImage: "trash") }
            }
    }
}

/// New task, or edit one (`editing`); the list is fixed when editing.
struct TaskSheet: View {
    @Bindable var google: GoogleStore
    @State var listId: String
    var editing: TaskItem?
    @Environment(\.dismiss) private var dismiss
    @State private var title = ""
    @State private var notes = ""
    @State private var hasDue = false
    @State private var due = Calendar.current.startOfDay(for: .now)
    @FocusState private var focused: Bool

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Title", text: $title, axis: .vertical)
                        .font(.title3.weight(.semibold))
                        .focused($focused)
                    if editing != nil {
                        TextField("Notes", text: $notes, axis: .vertical).lineLimit(1...6)
                    } else if !google.lists.isEmpty {
                        Picker("List", selection: $listId) {
                            ForEach(google.lists) { Text($0.title).tag($0.id) }
                        }
                    }
                }
                Section {
                    Toggle("Due date", isOn: $hasDue.animation()).tint(Palette.pink)
                    if hasDue {
                        // Quick picks, then the full calendar.
                        HStack(spacing: 8) {
                            quick("Today", 0)
                            quick("Tomorrow", 1)
                            quick("Next week", 7)
                        }
                        DatePicker("Date", selection: $due, displayedComponents: .date)
                            .datePickerStyle(.graphical)
                            .tint(Palette.pink)
                    }
                }
            }
            .scrollContentBackground(.hidden)
            .background(PastelBackground())
            .navigationTitle(editing == nil ? "New task" : "Edit task")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button(editing == nil ? "Add" : "Save") {
                        let t = title.trimmingCharacters(in: .whitespacesAndNewlines)
                        let d = hasDue ? due : nil
                        // Shown in the list at once; saved in the background.
                        if let e = editing {
                            Task { await google.updateTask(e, in: listId, title: t, notes: notes, due: d) }
                        } else {
                            Task { await google.addTask(list: listId, title: t, due: d) }
                        }
                        dismiss()
                    }
                    .bold()
                    .disabled(title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
            .onAppear {
                if let e = editing {
                    title = e.title
                    notes = e.notes ?? ""
                    if let d = e.dueDate { hasDue = true; due = d }
                } else {
                    focused = true
                }
            }
        }
        .presentationDetents([.medium, .large])
    }

    private func quick(_ label: LocalizedStringKey, _ days: Int) -> some View {
        let d = Calendar.current.date(byAdding: .day, value: days, to: Calendar.current.startOfDay(for: .now)) ?? .now
        let on = Calendar.current.isDate(d, inSameDayAs: due)
        return Button { withAnimation(.snappy) { due = d } } label: {
            Text(label)
                .font(.subheadline.weight(.semibold))
                .padding(.horizontal, 12)
                .padding(.vertical, 6)
                .foregroundStyle(on ? .white : .primary)
                .background {
                    if on { Capsule().fill(Palette.accent) } else { Capsule().fill(Palette.lavender.opacity(0.15)) }
                }
        }
        .buttonStyle(.borderless)
    }
}

struct TaskRow: View {
    let task: TaskItem
    var onToggle: () -> Void
    /// Ticked here first, so the little celebration plays before the task moves away.
    @State private var ticking = false
    @State private var bursts = 0

    private var checked: Bool { task.completed || ticking }

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Button(action: tap) {
                ZStack {
                    Circle().strokeBorder(checked ? Color.clear : Palette.lavender, lineWidth: 2)
                    if checked {
                        Circle().fill(Palette.accent)
                        Image(systemName: "checkmark").font(.caption.weight(.heavy)).foregroundStyle(.white).transition(.scale.combined(with: .opacity))
                    }
                }
                .frame(width: 24, height: 24)
                .keyframeAnimator(initialValue: 1.0, trigger: bursts) { v, s in v.scaleEffect(s) } keyframes: { _ in
                    CubicKeyframe(1.35, duration: 0.12)
                    SpringKeyframe(1.0, duration: 0.4, spring: .bouncy)
                }
                .overlay {
                    Color.clear.keyframeAnimator(initialValue: 0.0, trigger: bursts) { v, p in
                        v.overlay { Confetti(progress: p) }
                    } keyframes: { _ in
                        MoveKeyframe(0)
                        CubicKeyframe(1, duration: 0.65)
                    }
                }
                .animation(.spring(response: 0.3, dampingFraction: 0.6), value: checked)
            }
            .buttonStyle(.plain)
            .sensoryFeedback(.success, trigger: bursts)
            VStack(alignment: .leading, spacing: 4) {
                Text(task.title.isEmpty ? L("(No title)") : task.title)
                    .strikethrough(checked, color: .secondary)
                    .foregroundStyle(checked ? .secondary : .primary)
                if let n = task.notes, !n.isEmpty {
                    Text(n).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                }
                if let d = task.dueDate, !checked { dueChip(d) }
            }
            .animation(.easeOut(duration: 0.2), value: checked)
            Spacer(minLength: 0)
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
        .onChange(of: task.completed) { ticking = false }
    }

    private func tap() {
        guard !task.completed else { return onToggle() }
        ticking = true
        bursts += 1
        Task {
            try? await Task.sleep(for: .milliseconds(600))
            onToggle()
            // Untick if saving failed and the task stayed open.
            try? await Task.sleep(for: .seconds(2))
            ticking = false
        }
    }

    static func dueText(_ d: Date) -> String {
        d.formatted(.verbatim("\(year: .defaultDigits)/\(month: .twoDigits)/\(day: .twoDigits)", timeZone: .current, calendar: .current))
    }

    private func dueChip(_ d: Date) -> some View {
        let cal = Calendar.current
        let text = Self.dueText(d)
        if d < cal.startOfDay(for: .now) {
            return Chip(text: text, systemImage: "exclamationmark.circle", color: .red)
        } else if cal.isDateInToday(d) {
            return Chip(text: text, systemImage: "sun.max", color: Palette.pink)
        }
        return Chip(text: text, systemImage: "calendar", color: cal.isDateInTomorrow(d) ? Palette.lavender : Palette.sky)
    }
}

/// Hearts and sparkles popping out of the checkbox; `progress` runs 0 to 1.
struct Confetti: View {
    let progress: Double

    var body: some View {
        ZStack {
            ForEach(0..<8, id: \.self) { i in
                let a = Double(i) / 8 * 2 * .pi - .pi / 2
                let r = 8 + 18 * progress
                Image(systemName: i.isMultiple(of: 2) ? "heart.fill" : "sparkle")
                    .font(.system(size: i.isMultiple(of: 2) ? 7 : 8, weight: .bold))
                    .foregroundStyle(Palette.all[i % Palette.all.count])
                    .offset(x: cos(a) * r, y: sin(a) * r)
                    .scaleEffect(0.4 + 0.8 * progress)
                    .opacity(progress > 0 && progress < 1 ? sin(.pi * progress) * 1.4 : 0)
            }
        }
        .allowsHitTesting(false)
    }
}
