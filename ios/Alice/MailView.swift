import AliceKit
import SwiftUI

struct MailView: View {
    @Bindable var google: GoogleStore
    @State private var composing: Draft?

    var body: some View {
        NavigationStack {
            GoogleGate(google: google, group: "gmail", title: "Gmail") {
                List {
                    ForEach(google.inbox) { m in
                        NavigationLink(value: m) { MailRow(mail: m) }
                            .listRowInsets(EdgeInsets(top: 6, leading: 16, bottom: 6, trailing: 12))
                            .listRowBackground(Color.clear)
                            .listRowSeparatorTint(Palette.lavender.opacity(0.25))
                            .onAppear { if m.id == google.inbox.last?.id { Task { await google.loadMore() } } }
                            .swipeActions(edge: .leading, allowsFullSwipe: true) {
                                // Swipe right: read (or back to unread).
                                Button { Task { await google.setRead(m.id, m.unread) } } label: {
                                    Label(m.unread ? "Read" : "Unread", systemImage: m.unread ? "envelope.open.fill" : "envelope.badge.fill")
                                }
                                .tint(m.unread ? Palette.lavender : Palette.pink)
                            }
                    }
                    if google.loading.contains("more") {
                        ProgressView().frame(maxWidth: .infinity).listRowBackground(Color.clear)
                    }
                }
                .listStyle(.plain)
                .pastel()
                .overlay {
                    if google.inbox.isEmpty {
                        if !google.inboxLoaded || google.loading.contains("mail") {
                            ProgressView()
                        } else {
                            SoftEmpty(title: "Inbox zero", systemImage: "envelope.open", message: "All caught up ✨")
                        }
                    }
                }
                .refreshable { await google.loadInbox() }
                .task { if !google.inboxLoaded { await google.loadInbox() } }
                .navigationDestination(for: MailSummary.self) { MailDetailView(google: google, summary: $0) }
            }
            .navigationTitle("Inbox")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button { composing = Draft() } label: { Image(systemName: "square.and.pencil").foregroundStyle(Palette.accent) }
                }
            }
            .sheet(item: $composing) { ComposeView(google: google, draft: $0) }
            .googleErrors(google)
        }
    }
}

struct MailRow: View {
    let mail: MailSummary

    var body: some View {
        HStack(alignment: .center, spacing: 10) {
            Avatar(name: mail.senderName, key: MailAddress.address(mail.from), size: 30)
                .overlay(alignment: .topLeading) {
                    if mail.unread {
                        Circle().fill(Palette.pink).frame(width: 9, height: 9).overlay(Circle().stroke(.background, lineWidth: 1.5)).offset(x: -2, y: -2)
                    }
                }
            VStack(alignment: .leading, spacing: 1) {
                HStack(alignment: .firstTextBaseline) {
                    Text(mail.senderName).font(.subheadline.weight(mail.unread ? .bold : .medium)).lineLimit(1)
                    Spacer(minLength: 6)
                    Text(mail.sentAt.map(Fmt.short) ?? "").font(.caption2).foregroundStyle(mail.unread ? Palette.pink : .secondary)
                }
                Text(mail.displaySubject).font(.footnote.weight(mail.unread ? .semibold : .regular)).lineLimit(1)
                Text(mail.snippet).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
        }
        .padding(.vertical, 1)
    }
}

struct MailDetailView: View {
    @Bindable var google: GoogleStore
    let summary: MailSummary
    @State private var message: MailMessage?
    @State private var composing: Draft?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text(summary.displaySubject).font(.system(.title2, design: .rounded).weight(.bold)).textSelection(.enabled)
                HStack(spacing: 12) {
                    Avatar(name: summary.senderName, key: MailAddress.address(summary.from), size: 44)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(summary.senderName).font(.headline)
                        Text(MailAddress.address(summary.from)).font(.caption).foregroundStyle(.secondary)
                        if let to = message?.to, !to.isEmpty { Text("to \(to)").font(.caption2).foregroundStyle(.secondary).lineLimit(1) }
                    }
                    Spacer()
                    if let d = summary.sentAt { Text(d.formatted(date: .abbreviated, time: .shortened)).font(.caption).foregroundStyle(.secondary) }
                }
                .card()
                Group {
                    if let m = message {
                        Text(m.body.isEmpty ? summary.snippet : m.body)
                            .font(.body)
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    } else {
                        ProgressView().frame(maxWidth: .infinity)
                    }
                }
                .card(padding: 16)
            }
            .padding()
        }
        .background(PastelBackground())
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button { composing = Draft(replyTo: summary, message: message) } label: {
                    Image(systemName: "arrowshape.turn.up.left.fill").foregroundStyle(Palette.accent)
                }
                .accessibilityLabel("Reply")
            }
        }
        .sheet(item: $composing) { ComposeView(google: google, draft: $0) }
        .task { if message == nil { message = await google.open(summary.id) } }
    }
}

struct Draft: Identifiable {
    let id = UUID()
    var to = ""
    var cc = ""
    var subject = ""
    var body = ""
    var replyToId: String?

    init() {}

    init(replyTo s: MailSummary, message: MailMessage?) {
        let replyAddress = message?.replyTo.flatMap { $0.isEmpty ? nil : $0 } ?? s.from
        to = MailAddress.address(replyAddress)
        subject = s.subject.lowercased().hasPrefix("re:") ? s.subject : "Re: \(s.subject)"
        replyToId = s.id
        if let m = message {
            let when = m.sentAt?.formatted(date: .abbreviated, time: .shortened) ?? m.date
            let quoted = m.body.split(separator: "\n", omittingEmptySubsequences: false).prefix(40).map { "> \($0)" }.joined(separator: "\n")
            body = "\n\n\(when), \(s.senderName):\n\(quoted)"
        }
    }
}

struct ComposeView: View {
    @Bindable var google: GoogleStore
    @State var draft: Draft
    @Environment(\.dismiss) private var dismiss
    @FocusState private var bodyFocused: Bool
    @State private var showCc = false

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                field("To", text: $draft.to).keyboardType(.emailAddress).textInputAutocapitalization(.never).autocorrectionDisabled()
                if showCc || !draft.cc.isEmpty {
                    field("Cc", text: $draft.cc).keyboardType(.emailAddress).textInputAutocapitalization(.never).autocorrectionDisabled()
                }
                field("Subject", text: $draft.subject)
                TextEditor(text: $draft.body)
                    .focused($bodyFocused)
                    .scrollContentBackground(.hidden)
                    .padding(.horizontal, 12)
                    .frame(maxHeight: .infinity)
            }
            .background(PastelBackground())
            .navigationTitle(draft.replyToId == nil ? "New message" : "Reply")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) {
                    if !showCc && draft.cc.isEmpty { Button("Cc") { showCc = true } }
                }
                ToolbarItem(placement: .confirmationAction) {
                    if google.loading.contains("send") {
                        ProgressView()
                    } else {
                        Button {
                            Task {
                                if await google.send(to: draft.to, cc: draft.cc, subject: draft.subject, body: draft.body, replyTo: draft.replyToId) { dismiss() }
                            }
                        } label: { Image(systemName: "paperplane.fill").foregroundStyle(Palette.accent) }
                        .disabled(draft.to.trimmingCharacters(in: .whitespaces).isEmpty)
                    }
                }
            }
            .onAppear { if draft.replyToId != nil { bodyFocused = true } }
            .googleErrors(google)
        }
    }

    private func field(_ label: String, text: Binding<String>) -> some View {
        VStack(spacing: 0) {
            HStack {
                Text(label).foregroundStyle(.secondary).frame(width: 64, alignment: .leading)
                TextField("", text: text)
            }
            .padding(.horizontal)
            .padding(.vertical, 12)
            Divider().overlay(Palette.lavender.opacity(0.3))
        }
    }
}
