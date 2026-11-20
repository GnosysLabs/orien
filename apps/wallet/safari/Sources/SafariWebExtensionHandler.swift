import Foundation
import SafariServices

/// Answers the extension with what Orien Wallet shared: its address and
/// balance. Holds no keys; Orien Wallet does all signing.
class SafariWebExtensionHandler: NSObject, NSExtensionRequestHandling {
    func beginRequest(with context: NSExtensionContext) {
        var wallet: [String: Any] = [:]
        if let folder = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: "4PDUNTF69S.network.orien.wallet"),
           let data = try? Data(contentsOf: folder.appendingPathComponent("wallet.json")),
           let found = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            wallet = found
        }
        let response = NSExtensionItem()
        response.userInfo = [SFExtensionMessageKey: wallet]
        context.completeRequest(returningItems: [response], completionHandler: nil)
    }
}
