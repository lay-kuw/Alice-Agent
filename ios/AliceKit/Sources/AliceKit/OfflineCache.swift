import CryptoKit
import Foundation

/// The last data seen from one server, kept on this device so the app can show
/// it while the server is unreachable. Files use complete data protection
/// (unreadable while the phone is locked), are left out of backups, and are
/// deleted when the app disconnects from that server.
public struct OfflineCache: Sendable {
    let dir: URL

    public init?(server: String) {
        guard let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else { return nil }
        let key = SHA256.hash(data: Data(server.utf8)).prefix(12).map { String(format: "%02x", $0) }.joined()
        var dir = base.appendingPathComponent("offline", isDirectory: true).appendingPathComponent(key, isDirectory: true)
        do {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            var v = URLResourceValues()
            v.isExcludedFromBackup = true
            try dir.setResourceValues(v)
        } catch {
            return nil
        }
        self.dir = dir
    }

    public func load<T: Decodable>(_ type: T.Type, _ name: String) -> T? {
        guard let d = try? Data(contentsOf: dir.appendingPathComponent(name + ".json")) else { return nil }
        return try? JSONDecoder().decode(T.self, from: d)
    }

    /// Encoded here, written off the main thread.
    public func save<T: Encodable>(_ value: T, _ name: String) {
        guard let d = try? JSONEncoder().encode(value) else { return }
        let url = dir.appendingPathComponent(name + ".json")
        Task.detached(priority: .utility) {
            try? d.write(to: url, options: [.atomic, .completeFileProtection])
        }
    }

    public func clear() {
        try? FileManager.default.removeItem(at: dir)
    }
}
