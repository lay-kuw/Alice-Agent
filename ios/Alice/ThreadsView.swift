import AliceKit
import SwiftUI

/// The chat tab: the chat itself is the main screen; the thread list opens
/// from the top-left menu, settings and agents from the top right.
struct ChatTab: View {
    @Bindable var store: ChatStore
    let connectionName: String
    var onDisconnect: () -> Void
    @State private var showThreads = false
    @State private var showSettings = false
    @State private var showAgents = false

    var body: some View {
        NavigationStack {
            ChatView(store: store, openThreads: { showThreads = true }, openSettings: { showSettings = true }, openAgents: { showAgents = true })
        }
        .sheet(isPresented: $showThreads) {
            ThreadsSheet(store: store) { id in
                showThreads = false
                if let id { Task { await store.open(id) } } else { store.newChat() }
            }
        }
        .sheet(isPresented: $showSettings) {
            SettingsView(store: store, connectionName: connectionName, onDisconnect: onDisconnect) {
                showSettings = false
                showAgents = true
            }
        }
        .sheet(isPresented: $showAgents) {
            AgentsView(store: store) { thread in
                // "Open chat" from an agent: close the sheet and show it.
                showAgents = false
                Task { await store.open(thread) }
            }
        }
        // A tapped reply notification opens its chat.
        .onChange(of: Router.shared.thread, initial: true) { _, id in
            guard let id else { return }
            Router.shared.thread = nil
            showAgents = false
            showThreads = false
            Task { await store.open(id) }
        }
        #if DEBUG
        .onAppear {
            let env = ProcessInfo.processInfo.environment
            if env["ALICE_DEV_AGENTS"] != nil { showAgents = true }
            if env["ALICE_DEV_THREADS"] != nil { showThreads = true }
            if env["ALICE_DEV_SETTINGS"] != nil { showSettings = true }
        }
        .task(id: store.threads.count) {
            // ALICE_DEV_CHAT=1 opens the newest chat; a thread id opens that one.
            if let k = ProcessInfo.processInfo.environment["ALICE_DEV_CHAT"], store.currentThreadId == nil,
               let t = store.threads.first(where: { $0.id == k }) ?? (k.count == 12 ? nil : store.threads.first) {
                await store.open(t.id)
            }
        }
        #endif
        .alert("Error", isPresented: Binding(get: { store.lastError != nil }, set: { if !$0 { store.lastError = nil } })) {
            Button("OK", role: .cancel) {}
        } message: { Text(store.lastError ?? "") }
    }
}

/// All chats, starred first. Picking one (or New chat) hands it back.
struct ThreadsSheet: View {
    @Bindable var store: ChatStore
    /// nil = a new chat.
    var pick: (String?) -> Void
    @State private var renaming: ChatThread?
    @State private var newTitle = ""
    /// The tapped row. List selection (not buttons) opens chats, so a tap while a
    /// row's swipe actions are showing only closes them.
    @State private var picked: String?
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List(selection: $picked) {
                ForEach(store.threads) { t in
                    ThreadRow(thread: t, current: t.id == store.currentThreadId)
                        .tag(t.id)
                        .listRowBackground(Color.clear)
                        .listRowSeparatorTint(Palette.lavender.opacity(0.25))
                        // Right to left: delete. Left to right: rename.
                        .swipeActions(edge: .trailing) {
                            Button(role: .destructive) { Task { await store.delete(t.id) } } label: { Label { Text("Delete") } icon: { SwipeIcon(name: "trash") } }
                        }
                        // Press and hold: the row pops up with the same actions.
                        .contextMenu {
                            Button(t.isPinned ? "Unstar" : "Star", systemImage: t.isPinned ? "star.slash" : "star.fill") { Task { await store.setPinned(t.id, !t.isPinned) } }
                            Button("Rename", systemImage: "pencil") { newTitle = t.title; renaming = t }
                            Button("Delete", systemImage: "trash", role: .destructive) { Task { await store.delete(t.id) } }
                        }
                        .swipeActions(edge: .leading) {
                            Button { newTitle = t.title; renaming = t } label: { Label { Text("Rename") } icon: { SwipeIcon(name: "pencil") } }.tint(Palette.lavender)
                        }
                }
            }
            .listStyle(.plain)
            .onChange(of: picked) { _, id in
                guard let id else { return }
                picked = nil
                pick(id)
            }
            .pastel()
            .overlay {
                if store.threads.isEmpty {
                    SoftEmpty(title: "No chats yet", systemImage: "bubble.left.and.bubble.right", message: "Chats from the CLI, Web, Telegram and Discord appear here too.")
                }
            }
            .refreshable {
                // Let the spinner finish its animation even when the server answers at once.
                store.retryNow()
                async let done: Void = store.refresh()
                try? await Task.sleep(for: .milliseconds(500))
                await done
            }
            .navigationTitle("Chats")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Close") { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) {
                    Button { pick(nil) } label: { Image(systemName: "square.and.pencil").foregroundStyle(Palette.accent) }
                        .accessibilityLabel("New chat")
                }
            }
            .alert("Rename thread", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
                TextField("Title", text: $newTitle)
                Button("Save") { if let t = renaming { Task { await store.rename(t.id, to: newTitle) } } }
                Button("Cancel", role: .cancel) {}
            }
        }
        .presentationDetents([.large])
    }
}

/// Swipe actions scale their image to a fixed size, so the symbol is drawn
/// small in the middle of a larger clear canvas to keep it in proportion.
struct SwipeIcon: View {
    let name: String

    var body: some View { Image(uiImage: Self.image(name)) }

    static func image(_ name: String) -> UIImage {
        let cfg = UIImage.SymbolConfiguration(pointSize: 13, weight: .semibold)
        guard let sym = UIImage(systemName: name, withConfiguration: cfg)?.withTintColor(.white, renderingMode: .alwaysOriginal) else { return UIImage() }
        let side: CGFloat = 30
        return UIGraphicsImageRenderer(size: CGSize(width: side, height: side)).image { _ in
            sym.draw(at: CGPoint(x: (side - sym.size.width) / 2, y: (side - sym.size.height) / 2))
        }.withRenderingMode(.alwaysOriginal)
    }
}

struct ThreadRow: View {
    let thread: ChatThread
    var current = false

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: "sparkles")
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: 36, height: 36)
                .background(LinearGradient(colors: [Palette.color(for: thread.id), Palette.color(for: thread.id).opacity(0.65)], startPoint: .topLeading, endPoint: .bottomTrailing), in: RoundedRectangle(cornerRadius: 11, style: .continuous))
            Text(thread.displayTitle).font(.body.weight(current ? .bold : .medium)).foregroundStyle(.primary).lineLimit(1)
            Spacer(minLength: 0)
            if thread.isPinned {
                Image(systemName: "star.fill")
                    .font(.system(size: 13, weight: .bold))
                    .foregroundStyle(LinearGradient(colors: [Palette.lemon, Palette.pink], startPoint: .topLeading, endPoint: .bottomTrailing))
                    .shadow(color: Palette.pink.opacity(0.35), radius: 2, y: 1)
                    .accessibilityLabel("Starred")
            }
            if thread.generating == true { ProgressView().controlSize(.small).tint(Palette.pink) }
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }
}

/// Connection state as a little heart that beats while connected and turns
/// into a sleepy cloud while reconnecting. Only the scale animates, so it never
/// shifts in the toolbar.
struct ConnectionBadge: View {
    let connected: Bool

    var body: some View {
        if connected {
            Image(systemName: "heart.fill")
                .font(.system(size: 17, weight: .bold))
                .foregroundStyle(LinearGradient(colors: [Palette.pink, Palette.lavender], startPoint: .topLeading, endPoint: .bottomTrailing))
                .keyframeAnimator(initialValue: 1.0, repeating: true) { heart, scale in
                    heart.scaleEffect(scale)
                } keyframes: { _ in
                    // Lub-dub, then rest.
                    CubicKeyframe(1.22, duration: 0.14)
                    CubicKeyframe(1.0, duration: 0.14)
                    CubicKeyframe(1.14, duration: 0.12)
                    CubicKeyframe(1.0, duration: 0.2)
                    LinearKeyframe(1.0, duration: 0.9)
                }
                .frame(width: 24, height: 22)
        } else {
            // Same box as the heart, so nothing moves when the state changes.
            ZStack {
                Image(systemName: "cloud.fill")
                    .font(.system(size: 15, weight: .bold))
                    .foregroundStyle(Palette.peach)
                    .offset(x: -2, y: 2)
                Text("zZ")
                    .font(.system(size: 8, weight: .heavy, design: .rounded))
                    .foregroundStyle(Palette.lavender)
                    .offset(x: 9, y: -8)
            }
            .frame(width: 24, height: 22)
            .phaseAnimator([1.0, 0.55]) { v, o in v.opacity(o) } animation: { _ in .easeInOut(duration: 1.2) }
        }
    }
}
