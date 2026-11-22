import Foundation
import MailCraftCore
import Security

/// The core's view model (`crates/mobile/src/view.rs`), read from its JSON.
struct ViewModel: Decodable {
    var screen: String = "folders"
    var ready: Bool = false
    var accounts: [Account] = []
    var title: String = ""
    var messages: [Row] = []
    var total: Int = 0
    var message: Message?
    var compose: Compose?
    var searching: String?
    var adding: Bool = false
    var addingStep: String?
    var canAddAnyway: Bool = false
    var notice: String?
}

struct Folder: Decodable, Hashable {
    var name: String
    var path: String
    var role: String
    var unread: Int?
    var total: Int?
}

struct Account: Decodable, Identifiable {
    var id: String
    var email: String
    var name: String
    var state: String
    var error: String?
    var folders: [Folder]
}

struct Row: Decodable, Identifiable {
    var key: String
    var from: String?
    var subject: String?
    var preview: String?
    var date: Int?
    var unread: Bool?
    var flagged: Bool?
    var attachments: Bool?
    var id: String { key }
}

struct Attachment: Decodable { var name: String? }

struct Message: Decodable {
    var key: String
    var from: String?
    var to: String?
    var cc: String?
    var subject: String?
    var text: String?
    var attachments: [Attachment]?
}

struct Compose: Decodable {
    var id: Int
    var from: String
    var to: String
    var cc: String
    var subject: String
    var body: String
    var sending: Bool
    var error: String?
}

/// The Rust library's C functions (`MailCraftCore/include/mailcraftcore.h`): the strings they
/// return are read and given back at once.
private func take(_ p: UnsafeMutablePointer<CChar>?) -> String {
    guard let p else { return "" }
    defer { mailcraft_free(p) }
    return String(cString: p)
}

/// The app's one link to the core: starts it, sends the person's actions, and on a thread of
/// its own takes what comes back (the view to draw, passwords for the Keychain).
@MainActor
final class Mail: ObservableObject {
    @Published private(set) var view = ViewModel()
    @Published var problem: String?

    init() {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("mail")
        let tz = TimeZone.current
        let said = take(mailcraft_start(dir.path, Keychain.all(), Int32(tz.secondsFromGMT()), tz.identifier))
        if !said.isEmpty {
            problem = said
            return
        }
        let thread = Thread { [weak self] in self?.listen() }
        thread.name = "mailcraft-shell"
        thread.start()
        NotificationCenter.default.addObserver(forName: .NSSystemTimeZoneDidChange, object: nil, queue: .main) { _ in
            let tz = TimeZone.current
            _ = take(mailcraft_zone(Int32(tz.secondsFromGMT()), tz.identifier))
        }
    }

    nonisolated private func listen() {
        let decoder = JSONDecoder()
        while true {
            let m = take(mailcraft_poll(1000))
            guard !m.isEmpty, let data = m.data(using: .utf8),
                  let o = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { continue }
            switch o["kind"] as? String {
            case "render":
                guard let v = o["view"], let vd = try? JSONSerialization.data(withJSONObject: v),
                      let parsed = try? decoder.decode(ViewModel.self, from: vd) else { continue }
                Task { @MainActor in self.view = parsed }
            case "secret":
                if let e = o["entry"] as? String, let v = o["value"] as? String { Keychain.put(e, v) }
            case "forget":
                if let e = o["entry"] as? String { Keychain.remove(e) }
            case "notice":
                if let t = o["text"] as? String { Task { @MainActor in self.problem = t } }
            default:
                continue
            }
        }
    }

    func act(_ type: String, _ fields: [String: Any?] = [:]) {
        var o: [String: Any] = ["type": type]
        for (k, v) in fields { if let v { o[k] = v } }
        guard let data = try? JSONSerialization.data(withJSONObject: o), let json = String(data: data, encoding: .utf8) else { return }
        let said = take(mailcraft_send(json))
        if !said.isEmpty { problem = said }
    }

    func dismiss() {
        problem = nil
        act("dismissNotice")
    }
}

/// The passwords the core asks to keep, as generic passwords in the Keychain (this device
/// only, after its first unlock: the app checks mail in the background).
enum Keychain {
    private static let service = "MailCraft"

    static func put(_ entry: String, _ value: String) {
        let q: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: entry]
        SecItemDelete(q as CFDictionary)
        var add = q
        add[kSecValueData as String] = Data(value.utf8)
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(add as CFDictionary, nil)
    }

    static func remove(_ entry: String) {
        let q: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: entry]
        SecItemDelete(q as CFDictionary)
    }

    /// Every kept password, as the core's start takes them: `{"entry": "password"}`.
    static func all() -> String {
        let q: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service,
            kSecMatchLimit as String: kSecMatchLimitAll, kSecReturnAttributes as String: true, kSecReturnData as String: true,
        ]
        var out: CFTypeRef?
        var kept: [String: String] = [:]
        if SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let items = out as? [[String: Any]] {
            for i in items {
                if let e = i[kSecAttrAccount as String] as? String, let d = i[kSecValueData as String] as? Data, let v = String(data: d, encoding: .utf8) {
                    kept[e] = v
                }
            }
        }
        let data = (try? JSONSerialization.data(withJSONObject: kept)) ?? Data("{}".utf8)
        return String(data: data, encoding: .utf8) ?? "{}"
    }
}
