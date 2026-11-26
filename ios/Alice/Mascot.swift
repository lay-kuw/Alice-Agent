import AliceKit
import SwiftUI

/// Alice's face in a soft circle (chat avatar, comment bubbles).
struct MascotAvatar: View {
    var size: CGFloat = 36

    var body: some View {
        Image("MascotFace")
            .resizable()
            .scaledToFill()
            .frame(width: size, height: size)
            .clipShape(Circle())
            .overlay(Circle().strokeBorder(.white.opacity(0.7), lineWidth: 2))
            .shadow(color: Palette.lavender.opacity(0.45), radius: 6, y: 2)
            .accessibilityHidden(true)
    }
}

/// Alice large, floating on a cloud of soft color with a few sparkles.
struct MascotHero: View {
    var size: CGFloat = 200
    @State private var float = false

    var body: some View {
        ZStack {
            Circle().fill(Palette.lavender.opacity(0.35)).frame(width: size * 1.05).blur(radius: 30).offset(x: -size * 0.12)
            Circle().fill(Palette.pink.opacity(0.28)).frame(width: size * 0.8).blur(radius: 30).offset(x: size * 0.22, y: size * 0.1)
            Image("Mascot")
                .resizable()
                .scaledToFit()
                .frame(width: size, height: size)
                .shadow(color: Palette.lavender.opacity(0.45), radius: 14, y: 8)
                .offset(y: float ? -5 : 5)
            Sparkle(size: size * 0.13).offset(x: -size * 0.52, y: -size * 0.42)
            Sparkle(size: size * 0.08).offset(x: -size * 0.62, y: -size * 0.22)
            Sparkle(size: size * 0.09).offset(x: size * 0.55, y: size * 0.38)
        }
        .frame(height: size * 1.1)
        .onAppear { withAnimation(.easeInOut(duration: 2.4).repeatForever(autoreverses: true)) { float = true } }
        .accessibilityHidden(true)
    }
}

struct Sparkle: View {
    let size: CGFloat
    @State private var on = false

    var body: some View {
        Image(systemName: "sparkle")
            .font(.system(size: size, weight: .bold))
            .foregroundStyle(.white)
            .shadow(color: Palette.lemon.opacity(0.8), radius: 4)
            .opacity(on ? 1 : 0.45)
            .scaleEffect(on ? 1 : 0.8)
            .onAppear { withAnimation(.easeInOut(duration: 1.6).repeatForever(autoreverses: true).delay(Double.random(in: 0...1))) { on = true } }
    }
}

/// Alice's one-line comments, fetched per screen and kept while the facts stay the same.
@MainActor
@Observable
final class Blurbs {
    static let shared = Blurbs()
    private(set) var lines: [String: String] = [:]
    private var asked: [String: String] = [:]

    /// Turned off in Settings (no model calls at all then).
    static var enabled: Bool { UserDefaults.standard.object(forKey: "blurbs.enabled") as? Bool ?? true }

    /// Comments speak Alice's own language (from SOUL), whatever the app's language.
    func line(_ screen: String) -> String? { lines[screen] }

    func ask(_ screen: String, facts: String, client: AliceClient) async {
        guard Self.enabled else { return }
        let key = screen
        let sig = facts + "|" + Self.hour()
        guard asked[key] != sig else { return }
        asked[key] = sig
        if let text = try? await client.blurb(screen: screen, lang: "soul", facts: facts, now: Self.now()) {
            withAnimation(.snappy) { lines[key] = text }
        } else {
            asked[key] = nil
        }
    }

    /// The tone changed: ask again everywhere.
    func reset() {
        lines = [:]
        asked = [:]
    }

    private static func now() -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd HH:mm (EEEE)"
        return f.string(from: .now)
    }

    /// Lines follow the time of day, not every minute.
    private static func hour() -> String { String(Calendar.current.component(.hour, from: .now) / 3) }
}

/// Alice's face with her comment in a bubble, as at the top of each screen.
struct BlurbBubble: View {
    let screen: String
    /// Shown until (or instead of) Alice's own line.
    let fallback: LocalizedStringKey
    var blurbs = Blurbs.shared

    var body: some View {
        if UserDefaults.standard.object(forKey: "blurbs.enabled") as? Bool ?? true {
            HStack(alignment: .center, spacing: 10) {
                MascotAvatar(size: 46)
                Group {
                    if let line = blurbs.line(screen) { Text(line) } else { Text(fallback) }
                }
                .font(.subheadline.weight(.medium))
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(BubbleShape().fill(.background.opacity(0.75)))
                .overlay(BubbleShape().stroke(.white.opacity(0.6), lineWidth: 0.5))
                .shadow(color: Palette.lavender.opacity(0.18), radius: 8, y: 3)
                .contentTransition(.opacity)
            }
        }
    }
}

/// A rounded bubble with a small tail on the left.
struct BubbleShape: Shape {
    func path(in r: CGRect) -> Path {
        // One outline (bubble and tail joined), so the edge runs smoothly round the tail.
        let body = Path(roundedRect: r, cornerRadius: 18, style: .continuous)
        var tail = Path()
        tail.move(to: CGPoint(x: r.minX + 6, y: r.midY - 8))
        tail.addQuadCurve(to: CGPoint(x: r.minX - 7, y: r.midY + 1), control: CGPoint(x: r.minX - 1, y: r.midY - 2))
        tail.addQuadCurve(to: CGPoint(x: r.minX + 6, y: r.midY + 7), control: CGPoint(x: r.minX, y: r.midY + 6))
        tail.closeSubpath()
        return body.union(tail)
    }
}

enum Haptics {
    static func light() { UIImpactFeedbackGenerator(style: .light).impactOccurred() }
    static func medium() { UIImpactFeedbackGenerator(style: .medium).impactOccurred() }
    static func success() { UINotificationFeedbackGenerator().notificationOccurred(.success) }
    static func warning() { UINotificationFeedbackGenerator().notificationOccurred(.warning) }
}
