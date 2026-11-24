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
    @Environment(\.scenePhase) private var phase

    var body: some View {
        Group {
            if let store, let active {
                ThreadsView(store: store, connectionName: active.name) { disconnect() }
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
        active = c
    }

    private func disconnect() {
        store?.stopEvents()
        store = nil
        active = nil
    }
}
