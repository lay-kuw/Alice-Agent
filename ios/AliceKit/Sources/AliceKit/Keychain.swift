import Foundation
import Security

/// A saved server connection. Stored only in the Keychain.
public struct Connection: Codable, Identifiable, Hashable, Sendable {
    public var id: String { url }
    public var name: String
    public var url: String
    public var token: String

    public init(name: String, url: String, token: String) {
        self.name = name
        self.url = url
        self.token = token
    }
}

public enum ConnectionStore {
    static let service = "alice.connections"

    public static func load() -> [Connection] {
        let q: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: "all",
            kSecReturnData as String: true,
        ]
        var out: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let d = out as? Data else { return [] }
        return (try? JSONDecoder().decode([Connection].self, from: d)) ?? []
    }

    @discardableResult
    public static func save(_ list: [Connection]) -> Bool {
        guard let d = try? JSONEncoder().encode(list) else { return false }
        let q: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: "all",
        ]
        SecItemDelete(q as CFDictionary)
        var add = q
        add[kSecValueData as String] = d
        // Device-only, available after first unlock (needed for background reconnects).
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        return SecItemAdd(add as CFDictionary, nil) == errSecSuccess
    }
}
