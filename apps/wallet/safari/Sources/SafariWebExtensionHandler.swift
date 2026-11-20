import Foundation
import SafariServices

/// Answers the extension with what Orien Wallet tells it, on this Mac
/// only: the address and balance. Holds no keys; Orien Wallet does all
/// signing.
class SafariWebExtensionHandler: NSObject, NSExtensionRequestHandling {
    func beginRequest(with context: NSExtensionContext) {
        var request = URLRequest(url: URL(string: "http://127.0.0.1:47211/wallet")!, timeoutInterval: 3)
        request.setValue("1", forHTTPHeaderField: "X-Orien-Extension")
        URLSession.shared.dataTask(with: request) { data, _, _ in
            var wallet: [String: Any] = [:]
            if let data, let found = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                wallet = found
            }
            let response = NSExtensionItem()
            response.userInfo = [SFExtensionMessageKey: wallet]
            context.completeRequest(returningItems: [response], completionHandler: nil)
        }.resume()
    }
}
