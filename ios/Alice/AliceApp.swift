import AliceKit
import SwiftUI

@main
struct AliceApp: App {
    var body: some Scene {
        WindowGroup {
            RootView()
        }
    }
}

/// Chooses between the connection screen and the chat UI.
struct RootView: View {
    @State private var connections: [Connection] = ConnectionStore.load()
    @State private var active: Connection?
    @State private var store: ChatStore?
    @State private var google: GoogleStore?
    @Environment(\.scenePhase) private var phase

    var body: some View {
        Group {
            if let store, let google, let active {
                MainTabs(store: store, google: google, connectionName: active.name) { disconnect() }
            } else {
                ConnectView(connections: $connections) { connect($0) }
            }
        }
        .onAppear {
            if active == nil, connections.count == 1 { connect(connections[0]) }
        }
        .onChange(of: phase) { _, p in
            // Keep no stream open in the background; resync on return.
            if p == .background { store?.stopEvents() }
            if p == .active { store?.start() }
        }
    }

    private func connect(_ c: Connection) {
        guard let url = URL(string: c.url) else { return }
        let s = ChatStore(client: AliceClient(baseURL: url, token: c.token))
        s.start()
        store = s
        google = GoogleStore(client: s.client)
        active = c
    }

    private func disconnect() {
        store?.stopEvents()
        store = nil
        google = nil
        active = nil
    }
}

/// Chat plus the Google tabs (data comes from the Alice server, which holds the tokens).
struct MainTabs: View {
    let store: ChatStore
    let google: GoogleStore
    let connectionName: String
    var onDisconnect: () -> Void
    @SceneStorage("tab") private var tab = "chat"

    var body: some View {
        TabView(selection: $tab) {
            ThreadsView(store: store, connectionName: connectionName, onDisconnect: onDisconnect)
                .tabItem { Label("Chat", systemImage: "bubble.left.and.bubble.right.fill") }
                .tag("chat")
            CalendarView(google: google)
                .tabItem { Label("Calendar", systemImage: "calendar") }
                .tag("calendar")
            MailView(google: google)
                .tabItem { Label("Mail", systemImage: "envelope.fill") }
                .tag("mail")
            TasksView(google: google)
                .tabItem { Label("Tasks", systemImage: "checklist") }
                .tag("tasks")
        }
        .tint(Palette.lavender)
    }
}
