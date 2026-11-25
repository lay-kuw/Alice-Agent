import SwiftUI
import WebKit

/// An HTML email, drawn as its sender intended but kept private: no
/// JavaScript, and nothing is fetched from the network (tracking pixels,
/// remote images) until "Load images" is tapped. Links open in Safari.
struct HTMLMailView: View {
    let html: String
    @State private var height: CGFloat = 120
    @State private var remote = false

    /// Whether the mail refers to anything on the web worth offering to load.
    private var hasRemote: Bool { html.range(of: #"(src|background)\s*=\s*["']?https?:"#, options: [.regularExpression, .caseInsensitive]) != nil || html.range(of: "url(http", options: .caseInsensitive) != nil }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if hasRemote && !remote {
                Button { remote = true } label: {
                    Label("Load images", systemImage: "photo")
                        .font(.caption.weight(.semibold))
                        .padding(.horizontal, 10)
                        .padding(.vertical, 5)
                        .background(Palette.lavender.opacity(0.18), in: Capsule())
                }
                .buttonStyle(.plain)
                .foregroundStyle(Palette.lavender)
            }
            MailWebView(html: html, remote: remote, height: $height)
                .frame(height: height)
                .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        }
    }
}

private struct MailWebView: UIViewRepresentable {
    let html: String
    let remote: Bool
    @Binding var height: CGFloat

    private static let blockRemote = """
    [{"trigger":{"url-filter":"^(https?|ftp|wss?)://"},"action":{"type":"block"}}]
    """

    func makeCoordinator() -> Coordinator { Coordinator(height: $height) }

    func makeUIView(context: Context) -> WKWebView {
        let cfg = WKWebViewConfiguration()
        cfg.websiteDataStore = .nonPersistent()
        cfg.defaultWebpagePreferences.allowsContentJavaScript = false
        cfg.dataDetectorTypes = [.link, .phoneNumber]
        let v = WKWebView(frame: .zero, configuration: cfg)
        v.navigationDelegate = context.coordinator
        v.scrollView.isScrollEnabled = false
        v.scrollView.bounces = false
        v.isOpaque = false
        v.backgroundColor = .white
        return v
    }

    func updateUIView(_ v: WKWebView, context: Context) {
        let c = context.coordinator
        let key = "\(remote)\(html.hashValue)"
        guard c.loaded != key else { return }
        c.loaded = key
        c.remote = remote
        Task { @MainActor in
            let rules = v.configuration.userContentController
            rules.removeAllContentRuleLists()
            if !remote, let list = try? await WKContentRuleListStore.default().compileContentRuleList(forIdentifier: "alice.mail.block", encodedContentRuleList: Self.blockRemote) {
                rules.add(list)
            }
            v.loadHTMLString(Self.page(html), baseURL: nil)
        }
    }

    /// The mail inside a page that fits the phone and keeps a light look.
    static func page(_ html: String) -> String {
        """
        <!doctype html><html><head><meta charset="utf-8">
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <meta name="color-scheme" content="light">
        <style>
        html, body { margin: 0; padding: 0; background: #fff; }
        body { padding: 12px; font: -apple-system-body; color: #222; overflow-wrap: anywhere; -webkit-text-size-adjust: 100%; }
        img { max-width: 100% !important; height: auto !important; }
        pre { white-space: pre-wrap; }
        </style></head><body>\(html)</body></html>
        """
    }

    final class Coordinator: NSObject, WKNavigationDelegate {
        var loaded: String?
        private var height: Binding<CGFloat>

        init(height: Binding<CGFloat>) { self.height = height }

        var remote = false

        /// The page's own scripts are off; this one comes from the app. It hides
        /// images that were not loaded, shrinks fixed-width layouts to the
        /// screen, and reports the height.
        func webView(_ v: WKWebView, didFinish navigation: WKNavigation!) {
            let js = """
            (() => {
              if (!\(remote)) for (const i of document.images) if (/^(https?|ftp):/i.test(i.getAttribute('src') || '')) i.style.display = 'none';
              const b = document.body, w = document.documentElement.scrollWidth + 12, s = window.innerWidth / w;
              if (s < 1) b.style.zoom = s;
              return Math.ceil(Math.max(b.getBoundingClientRect().height, b.scrollHeight * (s < 1 ? s : 1)));
            })()
            """
            v.evaluateJavaScript(js) { [weak self] h, _ in
                guard let self, let h = h as? Double, h > 0 else { return }
                Task { @MainActor in self.height.wrappedValue = h }
            }
        }

        func webView(_ v: WKWebView, decidePolicyFor action: WKNavigationAction) async -> WKNavigationActionPolicy {
            if action.navigationType == .linkActivated, let url = action.request.url {
                if ["http", "https", "mailto", "tel"].contains(url.scheme?.lowercased() ?? "") { await UIApplication.shared.open(url) }
                return .cancel
            }
            return .allow
        }
    }
}
