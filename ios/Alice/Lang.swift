import SwiftUI

/// The app speaks Japanese in light mode and English in dark mode. SwiftUI
/// text follows the `locale` set at the root; strings built in code use `L`.
enum Lang {
    private static let key = "app.lang"

    /// "ja" or "en"; remembered for background work (notifications).
    static var code: String {
        get { UserDefaults.standard.string(forKey: key) ?? "ja" }
        set { UserDefaults.standard.set(newValue, forKey: key) }
    }

    static var isJapanese: Bool { code == "ja" }
    static var locale: Locale { Locale(identifier: code) }

    private static let bundles: [String: Bundle] = {
        var b: [String: Bundle] = [:]
        for c in ["ja", "en"] {
            if let p = Bundle.main.path(forResource: c, ofType: "lproj"), let x = Bundle(path: p) { b[c] = x }
        }
        return b
    }()

    /// A string from the catalog in the current language. English falls back
    /// to the source text when there is no English table.
    static func t(_ value: String.LocalizationValue) -> String {
        if let b = bundles[code] { return String(localized: value, bundle: b, locale: locale) }
        return String(localized: value, table: "Source", locale: locale)
    }
}

/// Shorthand for `Lang.t`.
func L(_ value: String.LocalizationValue) -> String { Lang.t(value) }

/// Sets the language from the appearance for everything below.
struct LanguageByAppearance: ViewModifier {
    @Environment(\.colorScheme) private var scheme

    func body(content: Content) -> some View {
        let code = scheme == .dark ? "en" : "ja"
        content
            .environment(\.locale, Locale(identifier: code))
            .onAppear { Lang.code = code }
            .onChange(of: scheme) { _, s in Lang.code = s == .dark ? "en" : "ja" }
    }
}
