import AliceKit
import SwiftUI

/// Home: Alice greets you, today's events and to-dos at a glance, and a box
/// to ask her something. Can be turned off in Settings.
struct HomeView: View {
    @Bindable var store: ChatStore
    @Bindable var google: GoogleStore
    let connectionName: String
    var onDisconnect: () -> Void
    /// Switch tabs ("chat", "calendar", "mail", "tasks").
    var go: (String) -> Void
    @State private var ask = ""
    @FocusState private var focused: Bool

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    greeting
                    tiles
                    if google.status?.enabled("calendar") == true { todayCard }
                    if google.status?.enabled("tasks") == true { todoCard }
                }
                .padding(.horizontal)
                .padding(.bottom, 20)
            }
            .scrollDismissesKeyboard(.interactively)
            .background(PastelBackground())
            .safeAreaInset(edge: .bottom) { askBar }
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button { Drawer.shared.open = true } label: { Image(systemName: "line.3.horizontal") }.accessibilityLabel("Chats")
                }
                ToolbarItem(placement: .principal) { Text("Alice Agent ✨").font(.headline.weight(.bold)) }
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        Button("Settings", systemImage: "gearshape") { Sheets.shared.settings = true }
                        Button("Agents", systemImage: "clock.badge.checkmark") { Sheets.shared.agents = true }
                    } label: { AppMenuIcon() }
                }
            }
            .task(id: "\(BlurbFacts.home(google))\(Blurbs.shared.enabled)") { await Blurbs.shared.ask("home", facts: BlurbFacts.home(google), client: store.client) }
        }
    }

    // MARK: parts

    private var greeting: some View {
        HStack(alignment: .center, spacing: 4) {
            Group {
                if let line = Blurbs.shared.line("home") { Text(line) } else { Text(Self.hello) }
            }
            .font(.system(.title3, design: .rounded).weight(.bold))
            .fixedSize(horizontal: false, vertical: true)
            .contentTransition(.opacity)
            .frame(maxWidth: .infinity, alignment: .leading)
            MascotHero(size: 130)
        }
        .padding(.top, 4)
    }

    private static var hello: LocalizedStringKey {
        switch Calendar.current.component(.hour, from: .now) {
        case 4..<11: "Good morning!\nWhat shall we do today?"
        case 11..<17: "Hello!\nHow is your day going?"
        default: "Good evening!\nHow was your day?"
        }
    }

    private var tiles: some View {
        HStack(spacing: 10) {
            tile("Chat", "ellipsis.bubble.fill", "chat")
            tile("Calendar", "calendar", "calendar")
            tile("Mail", "envelope.fill", "mail", dot: google.inbox.contains(where: \.unread))
            tile("To-do", "checkmark.square.fill", "tasks")
        }
    }

    private func tile(_ title: LocalizedStringKey, _ icon: String, _ tab: String, accent: Bool = false, dot: Bool = false) -> some View {
        Button {
            Haptics.light()
            // Chat opens on its main screen.
            if tab == "chat" { store.newChat() }
            go(tab)
        } label: {
            VStack(spacing: 6) {
                Image(systemName: icon)
                    .font(.system(size: 22, weight: .semibold))
                    .foregroundStyle(accent ? AnyShapeStyle(.white) : AnyShapeStyle(Palette.accent))
                    .overlay(alignment: .topTrailing) {
                        if dot { Circle().fill(Palette.pink).frame(width: 8, height: 8).offset(x: 5, y: -3) }
                    }
                Text(title).font(.caption.weight(.semibold)).foregroundStyle(accent ? .white : .primary).lineLimit(1).minimumScaleFactor(0.8)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 14)
            .background {
                if accent {
                    RoundedRectangle(cornerRadius: 18, style: .continuous).fill(Palette.accent).shadow(color: Palette.lavender.opacity(0.5), radius: 10, y: 4)
                }
            }
            .modifier(TileGlass(on: !accent))
        }
        .buttonStyle(.plain)
    }

    private var todayCard: some View {
        let events = google.events(on: .now)
        return VStack(alignment: .leading, spacing: 10) {
            cardHeader("Today's schedule", "calendar", tab: "calendar")
            if events.isEmpty {
                Text("Nothing planned ☕️").font(.subheadline).foregroundStyle(.secondary)
            }
            ForEach(events.prefix(4)) { e in
                HStack(spacing: 10) {
                    Circle().fill(Palette.event((e.id, e.colorId))).frame(width: 7, height: 7)
                    Text(e.allDay ? L("All day") : e.startDate.map(Fmt.time) ?? "")
                        .font(.subheadline.monospacedDigit())
                        .foregroundStyle(.secondary)
                        .frame(width: 52, alignment: .leading)
                    Text(e.displayTitle).font(.subheadline.weight(.semibold)).lineLimit(1)
                    Spacer(minLength: 0)
                }
            }
        }
        .padding(16)
        .glassCard(radius: 22)
    }

    private var todoCard: some View {
        let open = google.lists.flatMap { l in l.tasks.filter { !$0.completed }.map { (l.id, $0) } }
            .sorted { ($0.1.dueDate ?? .distantFuture) < ($1.1.dueDate ?? .distantFuture) }
        return VStack(alignment: .leading, spacing: 8) {
            cardHeader("To-do", "checkmark.square.fill", tab: "tasks")
            if open.isEmpty && google.tasksLoaded {
                Text("All done! 🌷").font(.subheadline).foregroundStyle(.secondary)
            }
            ForEach(open.prefix(4), id: \.1.id) { list, t in
                HStack(spacing: 10) {
                    Button {
                        Haptics.medium()
                        Task { await google.toggle(t, in: list) }
                    } label: {
                        Circle().strokeBorder(Palette.lavender, lineWidth: 2).frame(width: 22, height: 22)
                    }
                    .buttonStyle(.plain)
                    Text(t.title.isEmpty ? L("(No title)") : t.title).font(.subheadline).lineLimit(1)
                    Spacer(minLength: 0)
                    if let d = t.dueDate, Calendar.current.isDateInToday(d) {
                        Image(systemName: "sun.max.fill").font(.caption).foregroundStyle(Palette.pink)
                    }
                }
                .padding(.vertical, 2)
            }
        }
        .padding(16)
        .glassCard(radius: 22)
    }

    private func cardHeader(_ title: LocalizedStringKey, _ icon: String, tab: String) -> some View {
        HStack {
            Image(systemName: icon).foregroundStyle(Palette.accent)
            Text(title).font(.headline.weight(.bold))
            Spacer()
            Button { go(tab) } label: {
                HStack(spacing: 2) {
                    Text("See all")
                    Image(systemName: "chevron.right")
                }
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
        }
    }

    private var askBar: some View {
        HStack(spacing: 8) {
            TextField("Ask Alice…", text: $ask, axis: .vertical)
                .lineLimit(1...4)
                .focused($focused)
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .glassCard(radius: 22)
                .onSubmit(send)
            Button(action: send) {
                Image(systemName: "paperplane.fill")
                    .font(.system(size: 16, weight: .bold))
                    .foregroundStyle(.white)
                    .frame(width: 42, height: 42)
                    .background(ask.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? AnyShapeStyle(Color.secondary.opacity(0.35)) : AnyShapeStyle(Palette.accent), in: Circle())
            }
            .disabled(ask.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            .accessibilityLabel("Send")
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
    }

    /// A new chat with this question, in the Chat tab.
    private func send() {
        let text = ask.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        Haptics.light()
        ask = ""
        focused = false
        store.newChat()
        go("chat")
        Task {
            if !(await store.send(text)) { ask = text }
        }
    }
}

/// Frosted tile unless it is the highlighted one.
private struct TileGlass: ViewModifier {
    let on: Bool
    func body(content: Content) -> some View {
        if on { content.glassCard(radius: 18) } else { content }
    }
}
