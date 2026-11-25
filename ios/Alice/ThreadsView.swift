import AliceKit
import SwiftUI

/// Thread list (sidebar on iPad-sized layouts, root list on iPhone).
struct ThreadsView: View {
    @Bindable var store: ChatStore
    let connectionName: String
    var onDisconnect: () -> Void

    @State private var showChat = false
    @State private var renaming: ChatThread?
    @State private var newTitle = ""
    @State private var confirmDelete: ChatThread?
    /// The tapped row. List selection (not buttons) opens chats, so a tap while a
    /// row's swipe actions are showing only closes them.
    @State private var picked: String?

    var body: some View {
        NavigationStack {
            List(selection: $picked) {
                ForEach(store.threads) { t in
                    ThreadRow(thread: t)
                        .tag(t.id)
                        .listRowBackground(Color.clear)
                        .listRowSeparatorTint(Palette.lavender.opacity(0.25))
                        // Right to left: delete. Left to right: rename.
                        .swipeActions(edge: .trailing) {
                            Button(role: .destructive) { confirmDelete = t } label: { Label("Delete", systemImage: "trash") }
                        }
                        .swipeActions(edge: .leading) {
                            Button { newTitle = t.title; renaming = t } label: { Label("Rename", systemImage: "pencil") }.tint(Palette.lavender)
                        }
                }
            }
            .listStyle(.plain)
            .onChange(of: picked) { _, id in
                guard let id else { return }
                Task { await store.open(id) }
                showChat = true
                picked = nil
            }
            .pastel()
            .overlay {
                if store.threads.isEmpty {
                    SoftEmpty(title: "No chats yet", systemImage: "bubble.left.and.bubble.right", message: "Chats from the CLI, Web, Telegram and Discord appear here too.")
                }
            }
            .refreshable {
                // Let the spinner finish its animation even when the server answers at once.
                async let done: Void = store.refresh()
                try? await Task.sleep(for: .milliseconds(500))
                await done
            }
            .toolbarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Menu {
                        Label(store.connected ? "Connected" : "Reconnecting…", systemImage: store.connected ? "checkmark.circle" : "arrow.triangle.2.circlepath")
                        Button("Switch server", systemImage: "server.rack", action: onDisconnect)
                    } label: { ConnectionBadge(connected: store.connected) }
                    .accessibilityLabel(store.connected ? "Connected to \(connectionName)" : "Reconnecting")
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        store.newChat()
                        showChat = true
                    } label: { Image(systemName: "square.and.pencil").foregroundStyle(Palette.accent) }
                }
            }
            .navigationDestination(isPresented: $showChat) {
                ChatView(store: store)
            }
            #if DEBUG
            .task(id: store.threads.first?.id) {
                if ProcessInfo.processInfo.environment["ALICE_DEV_CHAT"] != nil, !showChat, let t = store.threads.first {
                    await store.open(t.id)
                    showChat = true
                }
            }
            #endif
            .alert("Rename thread", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
                TextField("Title", text: $newTitle)
                Button("Save") { if let t = renaming { Task { await store.rename(t.id, to: newTitle) } } }
                Button("Cancel", role: .cancel) {}
            }
            .confirmationDialog("Delete this thread? Memories learned from it are deleted too.", isPresented: Binding(get: { confirmDelete != nil }, set: { if !$0 { confirmDelete = nil } }), titleVisibility: .visible) {
                Button("Delete", role: .destructive) { if let t = confirmDelete { Task { await store.delete(t.id) } } }
            }
            .alert("Error", isPresented: Binding(get: { store.lastError != nil }, set: { if !$0 { store.lastError = nil } })) {
                Button("OK", role: .cancel) {}
            } message: { Text(store.lastError ?? "") }
        }
    }
}

struct ThreadRow: View {
    let thread: ChatThread

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: "sparkles")
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: 36, height: 36)
                .background(LinearGradient(colors: [Palette.color(for: thread.id), Palette.color(for: thread.id).opacity(0.65)], startPoint: .topLeading, endPoint: .bottomTrailing), in: RoundedRectangle(cornerRadius: 11, style: .continuous))
            Text(thread.displayTitle).font(.body.weight(.medium)).foregroundStyle(.primary).lineLimit(1)
            Spacer(minLength: 0)
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
        } else {
            Image(systemName: "cloud.fill")
                .font(.system(size: 16, weight: .bold))
                .foregroundStyle(Palette.peach)
                .overlay(alignment: .topTrailing) {
                    Text("z").font(.system(size: 9, weight: .heavy, design: .rounded)).foregroundStyle(Palette.lavender).offset(x: 5, y: -6)
                }
                .symbolEffect(.pulse)
        }
    }
}
