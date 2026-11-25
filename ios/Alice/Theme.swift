import SwiftUI

/// Alice's pastel palette (matches the TUI and Web accents).
enum Palette {
    static let lavender = Color(red: 0.72, green: 0.65, blue: 0.96)
    static let pink = Color(red: 0.97, green: 0.62, blue: 0.78)
    static let mint = Color(red: 0.55, green: 0.86, blue: 0.75)
    static let peach = Color(red: 1.0, green: 0.76, blue: 0.60)
    static let sky = Color(red: 0.57, green: 0.80, blue: 0.98)
    static let lemon = Color(red: 0.98, green: 0.86, blue: 0.48)

    static let all = [lavender, pink, mint, peach, sky, lemon]
    static let sunday = Color(red: 0.93, green: 0.45, blue: 0.50)
    static let saturday = Color(red: 0.40, green: 0.60, blue: 0.93)

    /// Soft red for Sundays, soft blue for Saturdays, nil for weekdays.
    static func weekday(_ d: Date) -> Color? {
        switch Calendar.current.component(.weekday, from: d) {
        case 1: sunday
        case 7: saturday
        default: nil
        }
    }
    static let accent = LinearGradient(colors: [lavender, pink], startPoint: .topLeading, endPoint: .bottomTrailing)

    /// A stable color for any string (sender, event id).
    static func color(for key: String) -> Color {
        let h = key.unicodeScalars.reduce(UInt32(5381)) { ($0 &* 33) &+ $1.value }
        return all[Int(h % UInt32(all.count))]
    }

    /// Google Calendar colorId -> a softened tone.
    static func event(_ e: (id: String, colorId: String?)) -> Color {
        switch e.colorId {
        case "1", "9": return lavender
        case "2", "10": return mint
        case "3": return Color(red: 0.80, green: 0.62, blue: 0.95)
        case "4", "11": return pink
        case "5": return lemon
        case "6": return peach
        case "7": return sky
        case "8": return Color(white: 0.75)
        default: return color(for: e.id)
        }
    }
}

/// Soft gradient behind every Google screen.
struct PastelBackground: View {
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        ZStack {
            (scheme == .dark ? Color(red: 0.08, green: 0.07, blue: 0.12) : Color(red: 0.99, green: 0.97, blue: 1.0))
            Circle().fill(Palette.lavender.opacity(scheme == .dark ? 0.22 : 0.30)).frame(width: 380).blur(radius: 90).offset(x: -150, y: -320)
            Circle().fill(Palette.pink.opacity(scheme == .dark ? 0.16 : 0.24)).frame(width: 340).blur(radius: 90).offset(x: 170, y: -120)
            Circle().fill(Palette.mint.opacity(scheme == .dark ? 0.12 : 0.20)).frame(width: 320).blur(radius: 100).offset(x: -60, y: 380)
        }
        .ignoresSafeArea()
    }
}

struct CardStyle: ViewModifier {
    var padding: CGFloat = 14

    func body(content: Content) -> some View {
        content
            .padding(padding)
            .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 20, style: .continuous).strokeBorder(.white.opacity(0.35), lineWidth: 0.5))
            .shadow(color: Palette.lavender.opacity(0.18), radius: 12, y: 6)
    }
}

extension View {
    func card(padding: CGFloat = 14) -> some View { modifier(CardStyle(padding: padding)) }

    /// Transparent list/scroll background so the pastel gradient shows through.
    func pastel() -> some View {
        scrollContentBackground(.hidden).background(PastelBackground())
    }
}

/// Initial in a pastel circle.
struct Avatar: View {
    let name: String
    let key: String
    var size: CGFloat = 40

    var body: some View {
        let c = Palette.color(for: key)
        Text(String(name.trimmingCharacters(in: .whitespaces).first ?? "?").uppercased())
            .font(.system(size: size * 0.42, weight: .bold, design: .rounded))
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(LinearGradient(colors: [c, c.opacity(0.7)], startPoint: .topLeading, endPoint: .bottomTrailing), in: Circle())
    }
}

/// Small rounded label.
struct Chip: View {
    let text: String
    var systemImage: String?
    var color: Color = Palette.lavender

    var body: some View {
        HStack(spacing: 4) {
            if let systemImage { Image(systemName: systemImage) }
            Text(text)
        }
        .font(.caption.weight(.semibold))
        .padding(.horizontal, 8)
        .padding(.vertical, 3)
        .foregroundStyle(color)
        .background(color.opacity(0.16), in: Capsule())
    }
}

/// Empty state with a soft icon.
struct SoftEmpty: View {
    let title: String
    let systemImage: String
    var message: String?

    var body: some View {
        VStack(spacing: 10) {
            Image(systemName: systemImage)
                .font(.system(size: 38, weight: .light))
                .foregroundStyle(Palette.accent)
            Text(title).font(.headline.weight(.semibold))
            if let message { Text(message).font(.subheadline).foregroundStyle(.secondary).multilineTextAlignment(.center) }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 40)
    }
}

enum Fmt {
    static func time(_ d: Date) -> String { d.formatted(date: .omitted, time: .shortened) }

    /// Mail list date: time today, weekday this week, else month/day.
    static func short(_ d: Date) -> String {
        let cal = Calendar.current
        if cal.isDateInToday(d) { return time(d) }
        if cal.isDateInYesterday(d) { return String(localized: "Yesterday") }
        if let days = cal.dateComponents([.day], from: d, to: .now).day, days < 7 { return d.formatted(.dateTime.weekday(.abbreviated)) }
        return d.formatted(.dateTime.month(.abbreviated).day())
    }
}
