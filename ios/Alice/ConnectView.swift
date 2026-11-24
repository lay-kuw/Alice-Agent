import AliceKit
import SwiftUI

/// Add, test and choose server connections (stored in the Keychain).
struct ConnectView: View {
    @Binding var connections: [Connection]
    var onConnect: (Connection) -> Void

    @State private var name = ""
    @State private var url = ""
    @State private var token = ""
    @State private var testing = false
    @State private var message: String?

    var body: some View {
        NavigationStack {
            Form {
                if !connections.isEmpty {
                    Section("Servers") {
                        ForEach(connections) { c in
                            Button {
                                onConnect(c)
                            } label: {
                                VStack(alignment: .leading) {
                                    Text(c.name).foregroundStyle(.primary)
                                    Text(c.url).font(.caption).foregroundStyle(.secondary)
                                }
                            }
                        }
                        .onDelete { idx in
                            connections.remove(atOffsets: idx)
                            ConnectionStore.save(connections)
                        }
                    }
                }
                Section {
                    TextField("Name (e.g. Home)", text: $name)
                    TextField("https://host.tailnet.ts.net", text: $url)
                        .keyboardType(.URL)
                        .textContentType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    SecureField("API token", text: $token)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                } header: {
                    Text("Add server")
                } footer: {
                    Text("Run `alice token` on the server to see the URL and token. For access away from home, use Tailscale or an HTTPS reverse proxy.")
                }
                Section {
                    Button {
                        Task { await test() }
                    } label: {
                        if testing { ProgressView() } else { Text("Connect") }
                    }
                    .disabled(url.isEmpty || token.isEmpty || testing)
                    if let message {
                        Text(message).font(.footnote).foregroundStyle(.red)
                    }
                }
            }
            .navigationTitle("Alice")
        }
    }

    private func test() async {
        let trimmed = url.trimmingCharacters(in: .whitespaces).trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        guard let u = URL(string: trimmed), u.scheme == "http" || u.scheme == "https" else {
            message = "Enter a valid http(s) URL"
            return
        }
        testing = true
        defer { testing = false }
        let client = AliceClient(baseURL: u, token: token.trimmingCharacters(in: .whitespaces))
        do {
            _ = try await client.threads()
            let c = Connection(name: name.isEmpty ? (u.host ?? "Alice") : name, url: trimmed, token: token.trimmingCharacters(in: .whitespaces))
            connections.removeAll { $0.url == c.url }
            connections.append(c)
            ConnectionStore.save(connections)
            message = nil
            name = ""; url = ""; token = ""
            onConnect(c)
        } catch {
            message = error.localizedDescription
        }
    }
}
