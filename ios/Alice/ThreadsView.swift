import AliceKit
import SwiftUI

/// Thread list (sidebar on iPad-sized layouts, root list on iPhone).
struct ThreadsView: View {
    @Bindable var store: ChatStore
    let connectionName: String
    var onDisconnect: () -> Void

    @State private var selection: String?
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
                    } label: {
                        HStack {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(t.displayTitle).lineLimit(1).foregroundStyle(.primary)
                                Text(Date(timeIntervalSince1970: Double(t.updatedAt) / 1000), style: .relative)
                                    .font(.caption).foregroundStyle(.secondary)
                            }
                            Spacer()
                            if t.generating == true { ProgressView().controlSize(.small) }
                        }
                    }
                    .swipeActions {
                        Button(role: .destructive) { confirmDelete = t } label: { Label("Delete", systemImage: "trash") }
                        Button { newTitle = t.title; renaming = t } label: { Label("Rename", systemImage: "pencil") }
                    }
                }
            }
            .overlay {
                if store.threads.isEmpty {
                    ContentUnavailableView("No threads", systemImage: "bubble.left.and.bubble.right", description: Text("Threads from the CLI, Web, Telegram and Discord appear here."))
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
                        Image(systemName: store.connected ? "circle.fill" : "circle.dashed").foregroundStyle(store.connected ? .green : .orange)
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        store.newChat()
                        showChat = true
                    } label: { Image(systemName: "square.and.pencil") }
                }
            }
            .navigationDestination(isPresented: $showChat) {
                ChatView(store: store)
            }
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
