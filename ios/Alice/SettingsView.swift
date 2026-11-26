import AliceKit
import SwiftUI

/// Settings from the chat's top-right menu: what the app shows, how Alice's
/// comments sound, notifications, agents and the server.
struct SettingsView: View {
    @Bindable var store: ChatStore
    let connectionName: String
    var onDisconnect: () -> Void
    var openAgents: () -> Void
    @Environment(\.dismiss) private var dismiss
    @AppStorage("home.enabled") private var homeEnabled = true
    @AppStorage("blurbs.enabled") private var blurbsEnabled = true
    @State private var tone = ""
    @State private var savedTone = ""
    @State private var notify: [Notifier.Kind: Bool] = [:]

    private static let presets: [String.LocalizationValue] = ["Cheerful and cute", "Calm and gentle", "Polite butler", "Cool big sister", "Kansai dialect"]

    var body: some View {
        NavigationStack {
            Form {
                Section("Display") {
                    Toggle("Home screen", isOn: $homeEnabled).tint(Palette.pink)
                    Toggle("Alice's comments", isOn: $blurbsEnabled).tint(Palette.pink)
                }
                if blurbsEnabled {
                    Section {
                        TextField("Her usual voice", text: $tone, axis: .vertical).lineLimit(1...3)
                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack(spacing: 8) {
                                ForEach(Self.presets.indices, id: \.self) { i in
                                    Button { tone = L(Self.presets[i]) } label: {
                                        Text(L(Self.presets[i])).font(.caption.weight(.semibold))
                                            .padding(.horizontal, 10).padding(.vertical, 5)
                                            .background(Palette.lavender.opacity(0.18), in: Capsule())
                                    }
                                    .buttonStyle(.plain)
                                    .foregroundStyle(Palette.lavender)
                                }
                            }
                        }
                        if tone != savedTone {
                            Button("Save tone") { save() }.bold()
                        }
                    } header: {
                        Text("Tone of Alice's comments")
                    } footer: {
                        Text("Shown at the top of Home, Chat, Calendar, Mail and To-do. Turn the comments off to save model use.")
                    }
                }
                Section("Alice") {
                    Button { openAgents() } label: { Label("Agents", systemImage: "clock.badge.checkmark") }
                    NavigationLink {
                        Form {
                            ForEach(Notifier.Kind.allCases) { k in
                                Toggle(isOn: Binding(get: { notify[k] ?? Notifier.enabled(k) }, set: { notify[k] = $0; Notifier.set(k, $0) })) { Text(k.title) }
                                    .tint(Palette.pink)
                            }
                        }
                        .scrollContentBackground(.hidden)
                        .background(PastelBackground())
                        .navigationTitle("Notifications")
                    } label: { Label("Notifications", systemImage: "bell") }
                }
                Section("Server") {
                    HStack {
                        Label(connectionName, systemImage: "server.rack")
                        Spacer()
                        Text(store.connected ? "Online" : "Reconnecting…").font(.caption).foregroundStyle(store.connected ? .green : .orange)
                    }
                    Button("Switch server", role: .destructive) {
                        dismiss()
                        onDisconnect()
                    }
                }
            }
            .scrollContentBackground(.hidden)
            .background(PastelBackground())
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { if tone != savedTone { save() }; dismiss() } } }
            .task {
                if let t = try? await store.client.blurbTone() {
                    tone = t
                    savedTone = t
                }
            }
        }
    }

    private func save() {
        let t = tone.trimmingCharacters(in: .whitespacesAndNewlines)
        savedTone = t
        Task {
            try? await store.client.setBlurbTone(t)
            Blurbs.shared.reset()
        }
    }

}
