import AliceKit
import BackgroundTasks
import SwiftUI
import WidgetKit

@main
struct AliceApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        WindowGroup {
            RootView()
        }
        .backgroundTask(.appRefresh(WidgetRefresh.id)) { await WidgetRefresh.run() }
    }
}

/// Keeps the widgets' snapshot fresh while the app is closed. iOS decides when
/// this runs (roughly every half hour to few hours, depending on use).
enum WidgetRefresh {
    static let id = "local.alice.refresh"
    private static let lastKey = "alice.lastConnection"

    static func schedule() {
        let r = BGAppRefreshTaskRequest(identifier: id)
        r.earliestBeginDate = Date(timeIntervalSinceNow: 30 * 60)
        try? BGTaskScheduler.shared.submit(r)
    }

    static func remember(_ c: Connection?) {
        UserDefaults.standard.set(c?.url, forKey: lastKey)
    }

    @MainActor
    static func run() async {
        schedule()
        let url = UserDefaults.standard.string(forKey: lastKey)
        guard let c = ConnectionStore.load().first(where: { $0.url == url }), let base = URL(string: c.url) else { return }
        let g = GoogleStore(client: AliceClient(baseURL: base, token: c.token), feedsWidgets: true)
        await g.prefetch()
        g.writeSnapshot()
        await Notifier.update(g)
        await Notifier.newReplies(g.client)
    }

    /// Widgets show nothing once the app is disconnected.
    static func clear() {
        WidgetSnapshot.clear()
        WidgetCenter.shared.reloadAllTimelines()
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
            #if DEBUG
            // Simulator runs: `SIMCTL_CHILD_ALICE_DEV_URL=… SIMCTL_CHILD_ALICE_DEV_TOKEN=… xcrun simctl launch …`
            let env = ProcessInfo.processInfo.environment
            if active == nil, let u = env["ALICE_DEV_URL"], let t = env["ALICE_DEV_TOKEN"] {
                connect(Connection(name: "Dev", url: u, token: t))
                return
            }
            #endif
            if active == nil, connections.count == 1 { connect(connections[0]) }
        }
        .onChange(of: phase) { _, p in
            // Keep no stream open in the background; resync on return.
            if p == .background {
                store?.stopEvents()
                if let store { Notifier.markChatsSeen(store.threads) }
                google?.writeSnapshot()
                WidgetRefresh.schedule()
            }
            if p == .active {
                store?.start()
                store?.retryNow()
                if let google { Task { await google.prefetch() } }
            }
        }
    }

    private func connect(_ c: Connection) {
        guard let url = URL(string: c.url) else { return }
        let s = ChatStore(client: AliceClient(baseURL: url, token: c.token))
        let g = GoogleStore(client: s.client, feedsWidgets: true)
        // Last data seen, shown at once and while the server is unreachable.
        let cache = OfflineCache(server: c.url)
        s.cache = cache
        g.cache = cache
        s.restore()
        g.restore()
        s.onReply = { t, m in Task { await Notifier.reply(in: t, m) } }
        s.start()
        store = s
        g.onChange = { [weak g] in if let g { Task { await Notifier.update(g) } } }
        Notifier.store = g
        google = g
        if c.name != "Dev" { Task { await Notifier.askOnce() } }
        if c.name != "Dev" { WidgetRefresh.remember(c) }
        active = c
    }

    private func disconnect() {
        store?.stopEvents()
        store?.cache?.clear()
        store = nil
        google = nil
        active = nil
        WidgetRefresh.remember(nil)
        WidgetRefresh.clear()
        Notifier.clear()
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
        #if DEBUG
        .onAppear { if let t = ProcessInfo.processInfo.environment["ALICE_DEV_TAB"] { tab = t } }
        #endif
        // Load every tab in the background so switching is instant.
        .task { await google.prefetch() }
        // Tapped notifications open their tab.
        .onChange(of: Router.shared.tab, initial: true) { _, t in
            if let t { tab = t; Router.shared.tab = nil }
        }
        // Widget taps open their tab (alice://calendar, alice://tasks).
        .onOpenURL { url in
            if let t = url.host(), ["chat", "calendar", "mail", "tasks"].contains(t) { tab = t }
        }
    }
}
