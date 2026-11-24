import AliceKit
import SwiftUI

/// Shows `content` only when Google is connected on the server and `group`
/// is turned on; otherwise explains how to set it up on the Mac.
struct GoogleGate<Content: View>: View {
    @Bindable var google: GoogleStore
    let group: String
    let title: String
    @ViewBuilder var content: () -> Content

    var body: some View {
        Group {
            if let s = google.status {
                if !s.ready {
                    setup(icon: "person.crop.circle.badge.exclamationmark", heading: "Connect Google on your Alice server",
                          steps: ["alice skill setup google-workspace", "alice skill login google-workspace"],
                          note: s.detail)
                } else if !s.enabled(group) {
                    setup(icon: "switch.2", heading: "\(title) is turned off",
                          steps: ["alice skill groups google-workspace", "alice skill login google-workspace"],
                          note: "Turn on “\(title)”, then reconnect so Google grants access. You can also do this in the Web UI under Settings → Skills.")
                } else {
                    content()
                        .safeAreaInset(edge: .top) {
                            if let e = google.loadError {
                                // Keep what is on screen; say the refresh failed.
                                Label("Couldn't refresh · pull to retry", systemImage: "exclamationmark.arrow.circlepath")
                                    .font(.caption.weight(.semibold))
                                    .foregroundStyle(Palette.pink)
                                    .padding(.horizontal, 12)
                                    .padding(.vertical, 6)
                                    .background(.ultraThinMaterial, in: Capsule())
                                    .help(e)
                                    .onTapGesture { google.loadError = nil }
                                    .transition(.move(edge: .top).combined(with: .opacity))
                            }
                        }
                        .animation(.snappy, value: google.loadError)
                }
            } else if google.loading.contains("status") {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity).background(PastelBackground())
            } else {
                setup(icon: "wifi.exclamationmark", heading: "Can't reach Alice", steps: [], note: google.loadError ?? "")
            }
        }
        .task { if google.status == nil { await google.loadStatus() } }
    }

    private func setup(icon: String, heading: String, steps: [String], note: String) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                Image(systemName: icon).font(.system(size: 40, weight: .light)).foregroundStyle(Palette.accent)
                Text(heading).font(.title3.weight(.bold))
                if !note.isEmpty { Text(note).font(.subheadline).foregroundStyle(.secondary) }
                ForEach(Array(steps.enumerated()), id: \.offset) { i, cmd in
                    HStack(alignment: .firstTextBaseline, spacing: 10) {
                        Text("\(i + 1)").font(.caption.bold()).foregroundStyle(.white).frame(width: 22, height: 22).background(Palette.accent, in: Circle())
                        Text(cmd).font(.system(.callout, design: .monospaced)).textSelection(.enabled)
                    }
                }
                Button {
                    Task { await google.loadStatus() }
                } label: {
                    Label("Check again", systemImage: "arrow.clockwise").frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .tint(Palette.lavender)
                .padding(.top, 6)
            }
            .card(padding: 20)
            .padding()
            .padding(.top, 40)
        }
        .background(PastelBackground())
    }
}

/// Error alert shared by the Google tabs.
struct GoogleErrorAlert: ViewModifier {
    @Bindable var google: GoogleStore

    func body(content: Content) -> some View {
        content.alert("Something went wrong", isPresented: Binding(get: { google.lastError != nil }, set: { if !$0 { google.lastError = nil } })) {
            Button("OK", role: .cancel) {}
        } message: { Text(google.lastError ?? "") }
    }
}

extension View {
    func googleErrors(_ g: GoogleStore) -> some View { modifier(GoogleErrorAlert(google: g)) }
}
