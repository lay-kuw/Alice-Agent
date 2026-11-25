import AliceKit
import SwiftUI

/// Scheduled agents: what Alice does on her own, when, and which chat gets it.
struct AgentsView: View {
    @Bindable var store: ChatStore
    var openThread: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var editing: AgentJob?
    @State private var adding = false

    var body: some View {
        NavigationStack {
            List {
                Text("Pick a time and how often, and Alice does it on her own. Results arrive in the chat you choose.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
                if let agents = store.agents {
                    if agents.isEmpty {
                        SoftEmpty(title: "No agents yet", systemImage: "moon.stars", message: "Tap ＋ to add one 🌙")
                            .listRowBackground(Color.clear)
                            .listRowSeparator(.hidden)
                    }
                    ForEach(agents) { a in
                        AgentRow(agent: a) { on in Task { await store.setAgentEnabled(a, on) } }
                            .contentShape(Rectangle())
                            .onTapGesture { editing = a }
                            .listRowBackground(Color.clear)
                            .listRowSeparatorTint(Palette.lavender.opacity(0.25))
                            .contextMenu {
                                Button("Run now", systemImage: "play.fill") { run(a) }
                                Button("Edit", systemImage: "pencil") { editing = a }
                                if let t = a.threadId { Button("Open chat", systemImage: "bubble.left") { openThread(t) } }
                                Button("Delete", systemImage: "trash", role: .destructive) { Task { await store.deleteAgent(a) } }
                            }
                            .swipeActions(edge: .trailing) {
                                Button(role: .destructive) { Task { await store.deleteAgent(a) } } label: { Label { Text("Delete") } icon: { SwipeIcon(name: "trash") } }
                            }
                            .swipeActions(edge: .leading) {
                                Button { run(a) } label: { Label { Text("Run now") } icon: { SwipeIcon(name: "play.fill") } }.tint(Palette.lavender)
                            }
                    }
                } else {
                    ProgressView().frame(maxWidth: .infinity).listRowBackground(Color.clear)
                }
            }
            .listStyle(.plain)
            .pastel()
            .refreshable { await store.loadAgents() }
            .task { await store.loadAgents() }
            .navigationTitle("Agents")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Done") { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) {
                    Button { adding = true } label: { Image(systemName: "plus.circle.fill").font(.title3).foregroundStyle(Palette.accent) }
                        .accessibilityLabel("New agent")
                }
            }
            .sheet(isPresented: $adding) { AgentEditor(store: store, agent: nil) }
            #if DEBUG
            .task(id: store.agents?.count) {
                let k = ProcessInfo.processInfo.environment["ALICE_DEV_AGENTS"]
                if k == "new", !adding { adding = true }
                if k == "edit", editing == nil, let a = store.agents?.first { editing = a }
            }
            #endif
            .sheet(item: $editing) { AgentEditor(store: store, agent: $0) }
        }
    }

    private func run(_ a: AgentJob) {
        Task {
            if let t = await store.runAgent(a) { openThread(t) }
        }
    }
}

struct AgentRow: View {
    let agent: AgentJob
    var setEnabled: (Bool) -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: "sparkles")
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: 36, height: 36)
                .background(LinearGradient(colors: [Palette.color(for: agent.id), Palette.color(for: agent.id).opacity(0.65)], startPoint: .topLeading, endPoint: .bottomTrailing), in: RoundedRectangle(cornerRadius: 11, style: .continuous))
            VStack(alignment: .leading, spacing: 3) {
                Text(agent.name).font(.body.weight(.semibold)).lineLimit(1)
                Text(AgentText.schedule(agent)).font(.caption).foregroundStyle(.secondary)
                Text(agent.prompt).font(.footnote).lineLimit(2).foregroundStyle(.primary.opacity(0.8))
                HStack(spacing: 6) {
                    Chip(text: AgentText.state(agent), systemImage: agent.enabled && agent.nextRun != nil ? "clock" : nil, color: agent.enabled ? Palette.lavender : .secondary)
                    Text("→ \(agent.threadTitle ?? String(localized: "New chat"))").font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                    if let s = agent.lastStatus, s != "ok" { Image(systemName: "exclamationmark.triangle.fill").font(.caption2).foregroundStyle(.orange).help(s) }
                }
            }
            Spacer(minLength: 0)
            Toggle("", isOn: Binding(get: { agent.enabled }, set: setEnabled)).labelsHidden().tint(Palette.pink)
        }
        .padding(.vertical, 6)
        .opacity(agent.enabled ? 1 : 0.6)
    }
}

enum AgentText {
    static func when(_ d: Date) -> String { d.formatted(.dateTime.month(.defaultDigits).day().weekday(.abbreviated).hour().minute()) }

    static func schedule(_ a: AgentJob) -> String {
        let (u, n) = AgentJob.split(a.everyMin)
        let time = a.start.formatted(date: .omitted, time: .shortened)
        switch u {
        case .once: return String(localized: "Once at \(when(a.start))")
        case .days where n == 1: return String(localized: "Every day at \(time)")
        case .weeks where n == 1: return String(localized: "Every \(a.start.formatted(.dateTime.weekday(.wide))) at \(time)")
        case .days: return String(localized: "Every \(n) days at \(time)")
        case .weeks: return String(localized: "Every \(n) weeks")
        case .hours: return String(localized: "Every \(n) h")
        case .minutes: return String(localized: "Every \(n) min")
        }
    }

    static func state(_ a: AgentJob) -> String {
        if !a.enabled { return String(localized: "Paused") }
        if let n = a.next { return String(localized: "Next \(when(n))") }
        return String(localized: "Done")
    }
}

/// New agent, or edit one.
struct AgentEditor: View {
    @Bindable var store: ChatStore
    let agent: AgentJob?
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var prompt = ""
    @State private var start = Self.nextHour()
    @State private var unit: AgentJob.Unit = .once
    @State private var count: Int64 = 1
    @State private var threadId: String = ""
    @State private var enabled = true
    @State private var saving = false

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Name", text: $name).font(.title3.weight(.semibold))
                    TextField("What to do (e.g. summarize today's events and unread mail)", text: $prompt, axis: .vertical)
                        .lineLimit(3...10)
                }
                Section {
                    DatePicker("Starts", selection: $start).tint(Palette.pink)
                    Picker("Repeat", selection: $unit) {
                        Text("Once").tag(AgentJob.Unit.once)
                        Text("Minutes").tag(AgentJob.Unit.minutes)
                        Text("Hours").tag(AgentJob.Unit.hours)
                        Text("Days").tag(AgentJob.Unit.days)
                        Text("Weeks").tag(AgentJob.Unit.weeks)
                    }
                    if unit != .once {
                        Stepper(value: $count, in: 1...(unit == .minutes ? 1440 : 365)) {
                            Text("Every \(count) \(unitName)")
                        }
                    }
                }
                Section {
                    Picker("Send to", selection: $threadId) {
                        Text("New chat").tag("")
                        ForEach(store.threads) { Text($0.displayTitle).tag($0.id) }
                    }
                    Toggle("Enabled", isOn: $enabled).tint(Palette.pink)
                }
                if let a = agent {
                    Section {
                        Button { Task { _ = await store.runAgent(a); dismiss() } } label: { Label("Run now", systemImage: "play.fill") }
                        Button(role: .destructive) { Task { await store.deleteAgent(a); dismiss() } } label: { Label("Delete", systemImage: "trash") }
                    }
                }
            }
            .scrollContentBackground(.hidden)
            .background(PastelBackground())
            .navigationTitle(agent == nil ? "New agent" : "Edit agent")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    if saving { ProgressView() } else {
                        Button(agent == nil ? "Add" : "Save") { save() }
                            .bold()
                            .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty || prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || tooOften)
                    }
                }
            }
            .onAppear {
                guard let a = agent else { return }
                name = a.name
                prompt = a.prompt
                start = a.start
                (unit, count) = AgentJob.split(a.everyMin)
                threadId = a.threadId ?? ""
                enabled = a.enabled
            }
        }
        .presentationDetents([.large])
    }

    private var everyMin: Int64 { unit == .once ? 0 : unit.minutes * count }
    private var tooOften: Bool { everyMin != 0 && everyMin < store.minEveryMin }

    private var unitName: String {
        switch unit {
        case .minutes: String(localized: "min")
        case .hours: String(localized: "h")
        case .days: String(localized: "days")
        case .weeks: String(localized: "weeks")
        case .once: ""
        }
    }

    private func save() {
        saving = true
        let f = AgentFields(name: name.trimmingCharacters(in: .whitespaces), prompt: prompt.trimmingCharacters(in: .whitespacesAndNewlines),
                            threadId: threadId.isEmpty ? nil : threadId, start: start, everyMin: everyMin, enabled: enabled)
        Task {
            if await store.saveAgent(id: agent?.id, f) { dismiss() }
            saving = false
        }
    }

    private static func nextHour() -> Date {
        let cal = Calendar.current
        let now = Date()
        return cal.date(byAdding: .hour, value: 1, to: cal.dateInterval(of: .hour, for: now)?.start ?? now) ?? now
    }
}
