import AliceKit
import PhotosUI
import SwiftUI
import UniformTypeIdentifiers

struct ChatView: View {
    @Bindable var store: ChatStore
    @State private var text = ""
    @State private var pending: [Attachment] = []
    @State private var photo: PhotosPickerItem?
    @State private var importing = false
    @State private var pickingPhoto = false
    @FocusState private var focused: Bool
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 14) {
                    ForEach(store.messages.filter { $0.role != "tool" && !($0.role == "user" && $0.client == "agent") && !AgentReply.isQuiet($0) }) { m in
                        MessageRow(message: m, store: store).id(m.id)
                    }
                    ForEach(store.currentApprovals) { a in
                        ApprovalCard(approval: a) { ok in Task { await store.resolve(a, approve: ok) } }
                    }
                    Color.clear.frame(height: 1).id("bottom")
                }
                .padding(.horizontal)
                .padding(.vertical, 12)
            }
            .scrollDismissesKeyboard(.interactively)
            .background(PastelBackground())
            .onChange(of: store.messages.last?.content) { _, _ in proxy.scrollTo("bottom", anchor: .bottom) }
            .onChange(of: store.messages.count) { _, _ in withAnimation { proxy.scrollTo("bottom", anchor: .bottom) } }
            .onAppear { proxy.scrollTo("bottom", anchor: .bottom) }
        }
        .safeAreaInset(edge: .bottom) { composer }
        .navigationTitle(store.currentThread?.displayTitle ?? "New chat")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) { modelMenu }
        }
        .onChange(of: store.currentThreadId) { old, new in
            // The thread was deleted (here or on another device).
            if old != nil, new == nil { dismiss() }
        }
        // A PhotosPicker placed inside a Menu never opens, so present it from here.
        .photosPicker(isPresented: $pickingPhoto, selection: $photo, matching: .images)
        .onChange(of: photo) { _, item in
            guard let item else { return }
            Task {
                // Photos are often HEIC; send JPEG so every model can read them.
                if let raw = try? await item.loadTransferable(type: Data.self),
                   let data = UIImage(data: raw)?.jpegData(compressionQuality: 0.85),
                   let a = await store.upload(name: "photo.jpg", mime: "image/jpeg", data: data) {
                    pending.append(a)
                }
                photo = nil
            }
        }
        .fileImporter(isPresented: $importing, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
            guard case .success(let urls) = result else { return }
            Task {
                for u in urls {
                    let ok = u.startAccessingSecurityScopedResource()
                    defer { if ok { u.stopAccessingSecurityScopedResource() } }
                    guard let data = try? Data(contentsOf: u) else { continue }
                    let mime = UTType(filenameExtension: u.pathExtension)?.preferredMIMEType ?? "application/octet-stream"
                    if let a = await store.upload(name: u.lastPathComponent, mime: mime, data: data) { pending.append(a) }
                }
            }
        }
    }

    private var modelMenu: some View {
        let current = store.currentThread?.model ?? store.pendingModel ?? ""
        let effective = current.isEmpty ? (store.models?.default ?? "") : current
        let (base, effort) = ModelID.split(effective)
        let info = store.models?.info(base)
        let all = store.models?.models ?? []
        let providers = all.reduce(into: [String]()) { acc, m in
            let p = String(m.split(separator: "/").first ?? "")
            if !acc.contains(p) { acc.append(p) }
        }
        return Menu {
            Button { Task { await store.setModel("") } } label: {
                Label("Default", systemImage: current.isEmpty ? "checkmark" : "")
            }
            ForEach(providers, id: \.self) { p in
                Section(p) {
                    ForEach(all.filter { $0.hasPrefix(p + "/") }, id: \.self) { m in
                        let mi = store.models?.info(m)
                        Button {
                            // Keep the reasoning level when the new model supports it.
                            let keep = effort.flatMap { e in mi?.efforts.contains(e) == true ? e : nil }
                            Task { await store.setModel(ModelID.join(m, keep)) }
                        } label: {
                            Label(mi?.name ?? String(m.dropFirst(p.count + 1)), systemImage: m == base && !current.isEmpty ? "checkmark" : "")
                        }
                    }
                }
            }
            if let info, !info.efforts.isEmpty {
                Section("Reasoning") {
                    Button { Task { await store.setModel(base) } } label: {
                        Label("Auto (\(info.defaultEffort ?? "-"))", systemImage: effort == nil ? "checkmark" : "")
                    }
                    ForEach(info.efforts, id: \.self) { e in
                        Button { Task { await store.setModel(ModelID.join(base, e)) } } label: {
                            Label(e.capitalized, systemImage: e == effort ? "checkmark" : "")
                        }
                    }
                }
            }
        } label: {
            ModelBadge(seed: base)
        }
        .menuIndicator(.hidden)
        .accessibilityLabel("Model: \(info?.name ?? base)\(effort.map { " · \($0)" } ?? "")")
    }

    private static let control: CGFloat = 40

    private var composer: some View {
        VStack(spacing: 6) {
            if !pending.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack {
                        ForEach(pending) { a in
                            HStack(spacing: 4) {
                                Image(systemName: a.isImage ? "photo" : "doc")
                                Text(a.name).lineLimit(1)
                                Button { pending.removeAll { $0.id == a.id } } label: { Image(systemName: "xmark.circle.fill") }
                            }
                            .font(.caption)
                            .padding(6)
                            .background(.thinMaterial, in: Capsule())
                        }
                    }
                }
            }
            HStack(alignment: .bottom, spacing: 8) {
                Menu {
                    Button { pickingPhoto = true } label: { Label("Photo", systemImage: "photo") }
                    Button { importing = true } label: { Label("File", systemImage: "doc") }
                } label: {
                    // Same height as a one-line field, so the row lines up.
                    Image(systemName: "plus")
                        .font(.system(size: 17, weight: .semibold))
                        .foregroundStyle(Palette.lavender)
                        .frame(width: Self.control, height: Self.control)
                        .background(.regularMaterial, in: Circle())
                }
                .accessibilityLabel("Attach")
                TextField("Message Alice…", text: $text, axis: .vertical)
                    .lineLimit(1...6)
                    .focused($focused)
                    // A one-line field is exactly as tall as the round buttons.
                    .padding(.horizontal, 14)
                    .padding(.vertical, 8)
                    .frame(minHeight: Self.control)
                    .background(.regularMaterial, in: RoundedRectangle(cornerRadius: Self.control / 2, style: .continuous))
                    // Tapping anywhere on the bubble (not only the text line) starts typing.
                    .contentShape(RoundedRectangle(cornerRadius: Self.control / 2, style: .continuous))
                    .onTapGesture { focused = true }
                if store.generating {
                    Button { Task { await store.stopGeneration() } } label: {
                        Image(systemName: "stop.fill")
                            .font(.system(size: 14, weight: .bold))
                            .foregroundStyle(.white)
                            .frame(width: Self.control, height: Self.control)
                            .background(Palette.pink, in: Circle())
                    }
                    .accessibilityLabel("Stop")
                } else {
                    Button {
                        let t = text
                        let ids = pending.map(\.id)
                        text = ""
                        pending = []
                        Task { await store.send(t, attachmentIds: ids) }
                    } label: {
                        let empty = text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && pending.isEmpty
                        Image(systemName: "arrow.up")
                            .font(.system(size: 17, weight: .bold))
                            .foregroundStyle(.white)
                            .frame(width: Self.control, height: Self.control)
                            .background(empty ? AnyShapeStyle(Color.secondary.opacity(0.35)) : AnyShapeStyle(Palette.accent), in: Circle())
                    }
                    .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && pending.isEmpty)
                    .accessibilityLabel("Send")
                }
            }
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(.ultraThinMaterial)
    }
}

struct MessageRow: View {
    let message: Message
    let store: ChatStore

    var body: some View {
        if message.role == "user" {
            HStack {
                Spacer(minLength: 40)
                VStack(alignment: .trailing, spacing: 6) {
                    ForEach(store.attachments(for: message.id)) { a in AttachmentChip(attachment: a, client: store.client) }
                    if !message.content.isEmpty {
                        Text(message.content)
                            .textSelection(.enabled)
                            .padding(.horizontal, 14)
                            .padding(.vertical, 9)
                            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 18))
                    }
                }
            }
        } else {
            VStack(alignment: .leading, spacing: 8) {
                if !message.content.isEmpty || message.isStreaming {
                    MarkdownView(text: message.content.isEmpty ? "…" : message.content)
                }
                ForEach(message.toolCalls ?? [], id: \.id) { call in
                    let result = store.toolResult(for: call.id)
                    let ok = result?.meta?["ok"]?.bool
                    Label {
                        Text("\(call.name) \(call.arguments.summary)").lineLimit(2)
                    } icon: {
                        Image(systemName: result == nil ? "gearshape" : (ok == false ? "xmark.circle" : "checkmark.circle"))
                    }
                    .font(.caption.monospaced())
                    .foregroundStyle(ok == false ? .red : .secondary)
                }
                if message.status == "error" {
                    Text(message.meta?["error"]?.string ?? "Generation failed").font(.caption).foregroundStyle(.red)
                } else if message.status == "cancelled" {
                    Text("Stopped").font(.caption).foregroundStyle(.secondary)
                } else if message.endsTurn, let model = message.meta?["model_name"]?.string ?? message.model {
                    Text(model).font(.caption2).foregroundStyle(.tertiary)
                }
            }
        }
    }
}

struct MarkdownView: View {
    let text: String

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(Array(MarkdownBlock.parse(text).enumerated()), id: \.offset) { _, block in
                switch block {
                case .paragraph(let s):
                    Text(MarkdownBlock.inline(s)).textSelection(.enabled)
                case .heading(let level, let s):
                    Text(MarkdownBlock.inline(s)).font(level == 1 ? .title2.bold() : level == 2 ? .title3.bold() : .headline)
                case .listItem(let marker, let s):
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Text(marker).foregroundStyle(.secondary)
                        Text(MarkdownBlock.inline(s)).textSelection(.enabled)
                    }
                case .quote(let s):
                    Text(MarkdownBlock.inline(s)).italic().foregroundStyle(.secondary)
                        .padding(.leading, 10)
                        .overlay(alignment: .leading) { Rectangle().fill(.quaternary).frame(width: 3) }
                case .code(let lang, let code):
                    CodeBlock(language: lang, code: code)
                case .table(let header, let align, let rows):
                    TableBlock(header: header, align: align, rows: rows)
                }
            }
        }
    }
}

struct CodeBlock: View {
    let language: String
    let code: String
    @State private var copied = false

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(language).font(.caption2).foregroundStyle(.secondary)
                Spacer()
                Button(copied ? "Copied" : "Copy") {
                    UIPasteboard.general.string = code
                    copied = true
                }
                .font(.caption)
            }
            ScrollView(.horizontal, showsIndicators: false) {
                Text(code).font(.system(.footnote, design: .monospaced)).textSelection(.enabled)
            }
        }
        .padding(10)
        .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 10))
    }
}

struct ApprovalCard: View {
    let approval: Approval
    var decide: (Bool) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label("Allow \(approval.tool)?", systemImage: "exclamationmark.shield").font(.headline)
            Text(approval.arguments.summary).font(.caption.monospaced()).foregroundStyle(.secondary).lineLimit(6)
            if !approval.client.isEmpty, approval.client != "ios" {
                Text("Requested from \(approval.client)").font(.caption2).foregroundStyle(.tertiary)
            }
            HStack {
                Button("Allow") { decide(true) }.buttonStyle(.borderedProminent)
                Button("Deny", role: .destructive) { decide(false) }.buttonStyle(.bordered)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 12))
    }
}

struct AttachmentChip: View {
    let attachment: Attachment
    let client: AliceClient
    @State private var image: UIImage?

    var body: some View {
        Group {
            if let image {
                Image(uiImage: image).resizable().scaledToFit().frame(maxWidth: 200, maxHeight: 160).clipShape(RoundedRectangle(cornerRadius: 10))
            } else {
                Label(attachment.name, systemImage: attachment.isImage ? "photo" : "doc").font(.caption)
            }
        }
        .task {
            // Loaded into memory only; never written to disk.
            guard attachment.isImage, image == nil, let d = try? await client.attachmentData(attachment.id) else { return }
            image = UIImage(data: d)
        }
    }
}

/// A Markdown table as a rounded card; wide tables scroll sideways.
struct TableBlock: View {
    let header: [String]
    let align: [TableAlign]
    let rows: [[String]]

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            Grid(alignment: .leading, horizontalSpacing: 0, verticalSpacing: 0) {
                GridRow {
                    ForEach(header.indices, id: \.self) { i in
                        cell(header[i], i).font(.subheadline.weight(.semibold))
                    }
                }
                .background(Palette.lavender.opacity(0.18))
                ForEach(rows.indices, id: \.self) { r in
                    Divider().overlay(Palette.lavender.opacity(0.2))
                    GridRow {
                        ForEach(header.indices, id: \.self) { i in
                            cell(i < rows[r].count ? rows[r][i] : "", i).font(.subheadline)
                        }
                    }
                    .background(r % 2 == 1 ? Palette.lavender.opacity(0.06) : .clear)
                }
            }
            .background(.regularMaterial)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Palette.lavender.opacity(0.3), lineWidth: 0.5))
        }
    }

    private func cell(_ s: String, _ i: Int) -> some View {
        let a = i < align.count ? align[i] : .left
        return Text(MarkdownBlock.inline(s))
            .textSelection(.enabled)
            .multilineTextAlignment(a == .right ? .trailing : a == .center ? .center : .leading)
            .frame(minWidth: 44, maxWidth: 260, alignment: a == .right ? .trailing : a == .center ? .center : .leading)
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .gridColumnAlignment(a == .right ? .trailing : a == .center ? .center : .leading)
    }
}

/// The model button: a simple sparkle.
struct ModelBadge: View {
    let seed: String

    var body: some View {
        // An image (not text), so the toolbar gives it a round button like the others.
        Image(systemName: "sparkle")
            .font(.system(size: 18, weight: .medium))
            .foregroundStyle(Palette.accent)
    }
}
