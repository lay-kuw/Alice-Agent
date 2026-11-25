import AliceKit
import SwiftUI

struct TasksView: View {
    @Bindable var google: GoogleStore
    /// The list shown, or nil for every list grouped by list.
    @AppStorage("tasks.list") private var listId = ""
    @State private var showCompleted = false
    @State private var adding = false

    var body: some View {
        NavigationStack {
            GoogleGate(google: google, group: "tasks", title: "Tasks") {
                List {
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
                .listStyle(.plain)
                .pastel()
                .overlay { if !google.tasksLoaded { ProgressView() } }
                .refreshable { await google.loadTasks() }
                .task { if !google.tasksLoaded { await google.loadTasks() } }
            }
            .navigationTitle("Tasks")
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
                NewTaskSheet(google: google, listId: selected?.id ?? google.lists.first?.id ?? "")
            }
            .googleErrors(google)
        }
    }

    /// The chosen list; nil means All.
    private var selected: TaskList? { google.lists.first { $0.id == listId } }

    private var shown: [TaskList] { selected.map { [$0] } ?? google.lists }

    private var listChips: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                chip(String(localized: "All"), id: "", count: google.lists.reduce(0) { $0 + $1.tasks.filter { !$0.completed }.count })
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
            .listRowBackground(Color.clear)
            .listRowSeparatorTint(Palette.lavender.opacity(0.2))
            .swipeActions {
                Button(role: .destructive) { Task { await google.deleteTask(t, in: list) } } label: { Label("Delete", systemImage: "trash") }
            }
    }
}

struct NewTaskSheet: View {
    @Bindable var google: GoogleStore
    @State var listId: String
    @Environment(\.dismiss) private var dismiss
    @State private var title = ""
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
                    if !google.lists.isEmpty {
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
            .navigationTitle("New task")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add") {
                        let t = title.trimmingCharacters(in: .whitespacesAndNewlines)
                        let d = hasDue ? due : nil
                        // Shown in the list at once; saved in the background.
                        Task { await google.addTask(list: listId, title: t, due: d) }
                        dismiss()
                    }
                    .bold()
                    .disabled(title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
            .onAppear { focused = true }
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

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Button(action: onToggle) {
                ZStack {
                    Circle().strokeBorder(task.completed ? Color.clear : Palette.lavender, lineWidth: 2)
                    if task.completed {
                        Circle().fill(Palette.accent)
                        Image(systemName: "checkmark").font(.caption.weight(.heavy)).foregroundStyle(.white).transition(.scale.combined(with: .opacity))
                    }
                }
                .frame(width: 24, height: 24)
                .animation(.spring(response: 0.3, dampingFraction: 0.6), value: task.completed)
            }
            .buttonStyle(.plain)
            .sensoryFeedback(.success, trigger: task.completed) { _, new in new }
            VStack(alignment: .leading, spacing: 4) {
                Text(task.title.isEmpty ? String(localized: "(No title)") : task.title)
                    .strikethrough(task.completed, color: .secondary)
                    .foregroundStyle(task.completed ? .secondary : .primary)
                if let n = task.notes, !n.isEmpty {
                    Text(n).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                }
                if let d = task.dueDate, !task.completed { dueChip(d) }
            }
            Spacer(minLength: 0)
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }

    private func dueChip(_ d: Date) -> some View {
        let cal = Calendar.current
        let today = cal.startOfDay(for: .now)
        if d < today {
            return Chip(text: d.formatted(.dateTime.month(.abbreviated).day()), systemImage: "exclamationmark.circle", color: .red)
        } else if cal.isDateInToday(d) {
            return Chip(text: String(localized: "Today"), systemImage: "sun.max", color: Palette.pink)
        } else if cal.isDateInTomorrow(d) {
            return Chip(text: String(localized: "Tomorrow"), systemImage: "calendar", color: Palette.lavender)
        }
        return Chip(text: d.formatted(.dateTime.month(.abbreviated).day()), systemImage: "calendar", color: Palette.sky)
    }
}
