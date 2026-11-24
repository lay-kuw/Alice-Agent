import AliceKit
import SwiftUI

struct TasksView: View {
    @Bindable var google: GoogleStore
    @State private var listId: String?
    @State private var newTitle = ""
    @State private var due: Date?
    @State private var showCompleted = false
    @FocusState private var adding: Bool

    var body: some View {
        NavigationStack {
            GoogleGate(google: google, group: "tasks", title: "Tasks") {
                List {
                    if google.lists.count > 1 { listChips.listRowBackground(Color.clear).listRowSeparator(.hidden) }
                    addBar.listRowBackground(Color.clear).listRowSeparator(.hidden)
                    if let list = current {
                        let open = list.tasks.filter { !$0.completed }
                        let done = list.tasks.filter(\.completed)
                        if open.isEmpty && google.tasksLoaded {
                            SoftEmpty(title: "All done!", systemImage: "checkmark.seal", message: "Enjoy your free time 🌷")
                                .listRowBackground(Color.clear).listRowSeparator(.hidden)
                        }
                        ForEach(open) { t in row(t, list: list.id) }
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
                            if showCompleted { ForEach(done) { t in row(t, list: list.id) } }
                        }
                    }
                }
                .listStyle(.plain)
                .pastel()
                .overlay { if !google.tasksLoaded { ProgressView() } }
                .refreshable { await google.loadTasks() }
                .task { if !google.tasksLoaded { await google.loadTasks() } }
            }
            .navigationTitle(current?.title ?? String(localized: "Tasks"))
            .googleErrors(google)
        }
    }

    private var current: TaskList? {
        google.lists.first { $0.id == listId } ?? google.lists.first
    }

    private var listChips: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(google.lists) { l in
                    let sel = l.id == current?.id
                    Button {
                        withAnimation(.snappy) { listId = l.id }
                    } label: {
                        Text(l.title)
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
            }
        }
    }

    private var addBar: some View {
        HStack(spacing: 10) {
            Image(systemName: "plus.circle.fill").font(.title2).foregroundStyle(Palette.accent)
            TextField("Add a task", text: $newTitle)
                .focused($adding)
                .submitLabel(.done)
                .onSubmit(add)
            Menu {
                Button("Today") { due = Calendar.current.startOfDay(for: .now) }
                Button("Tomorrow") { due = Calendar.current.date(byAdding: .day, value: 1, to: Calendar.current.startOfDay(for: .now)) }
                Button("Next week") { due = Calendar.current.date(byAdding: .day, value: 7, to: Calendar.current.startOfDay(for: .now)) }
                if due != nil { Button("No date", role: .destructive) { due = nil } }
            } label: {
                if let due {
                    Chip(text: due.formatted(.dateTime.month(.abbreviated).day()), systemImage: "calendar", color: Palette.pink)
                } else {
                    Image(systemName: "calendar.badge.plus").foregroundStyle(.secondary)
                }
            }
        }
        .card(padding: 12)
    }

    private func add() {
        let title = newTitle.trimmingCharacters(in: .whitespaces)
        guard !title.isEmpty, let list = current?.id else { return }
        newTitle = ""
        let d = due
        due = nil
        Task { await google.addTask(list: list, title: title, due: d) }
        adding = true
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
