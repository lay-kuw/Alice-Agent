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

    var body: some View {
        NavigationStack {
            List {
                ForEach(store.threads) { t in
                    Button {
                        Task { await store.open(t.id) }
                        showChat = true
                    } label: { ThreadRow(thread: t) }
                    .buttonStyle(.plain)
                    .listRowBackground(Color.clear)
                    .listRowSeparatorTint(Palette.lavender.opacity(0.25))
                    .swipeActions {
                        Button(role: .destructive) { confirmDelete = t } label: { Label("Delete", systemImage: "trash") }
                        Button { newTitle = t.title; renaming = t } label: { Label("Rename", systemImage: "pencil") }.tint(Palette.lavender)
                    }
                }
            }
            .listStyle(.plain)
            .pastel()
            .overlay {
                if store.threads.isEmpty {
                    SoftEmpty(title: "No chats yet", systemImage: "bubble.left.and.bubble.right", message: "Chats from the CLI, Web, Telegram and Discord appear here too.")
                }
            }
            .refreshable { await store.refresh() }
            .navigationTitle(connectionName)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Menu {
                        Label(store.connected ? "Connected" : "Reconnecting…", systemImage: store.connected ? "checkmark.circle" : "arrow.triangle.2.circlepath")
                        Button("Switch server", systemImage: "server.rack", action: onDisconnect)
                    } label: {
                        Circle().fill(store.connected ? Palette.mint : Palette.peach).frame(width: 10, height: 10)
                            .shadow(color: (store.connected ? Palette.mint : Palette.peach).opacity(0.8), radius: 4)
                    }
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
            VStack(alignment: .leading, spacing: 3) {
                Text(thread.displayTitle).font(.body.weight(.medium)).foregroundStyle(.primary).lineLimit(1)
                Text(Date(timeIntervalSince1970: Double(thread.updatedAt) / 1000), style: .relative)
                    .font(.caption).foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
            if thread.generating == true { ProgressView().controlSize(.small).tint(Palette.pink) }
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }
}
