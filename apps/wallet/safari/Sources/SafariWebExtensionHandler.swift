import Foundation
import SafariServices

/// Passes the extension's questions to Orien Wallet, running on this Mac,
/// and its answers back: the wallet's address and balance, a payment's
/// fee, and payments, which Orien Wallet approves with Touch ID or the
/// password. Holds no keys; Orien Wallet does all signing.
class SafariWebExtensionHandler: NSObject, NSExtensionRequestHandling {
    func beginRequest(with context: NSExtensionContext) {
        let item = context.inputItems.first as? NSExtensionItem
        let message = item?.userInfo?[SFExtensionMessageKey] as? [String: Any] ?? [:]
        let ask = message["ask"] as? String ?? "wallet"
        var request: URLRequest
        switch ask {
        case "people":
            request = URLRequest(url: URL(string: "http://127.0.0.1:47211/people")!, timeoutInterval: 3)
        case "quote", "send", "search":
            request = URLRequest(url: URL(string: "http://127.0.0.1:47211/\(ask)")!, timeoutInterval: ask == "send" ? 180 : 8)
            request.httpMethod = "POST"
            request.httpBody = try? JSONSerialization.data(withJSONObject: message)
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        default:
            request = URLRequest(url: URL(string: "http://127.0.0.1:47211/wallet")!, timeoutInterval: 3)
        }
        request.setValue("1", forHTTPHeaderField: "X-Orien-Extension")
        URLSession.shared.dataTask(with: request) { data, _, error in
            var answer: Any = [String: Any]()
            if let data, let found = try? JSONSerialization.jsonObject(with: data) {
                answer = found
            } else if error != nil {
                answer = ["closed": true]
            }
            let response = NSExtensionItem()
            response.userInfo = [SFExtensionMessageKey: answer]
            context.completeRequest(returningItems: [response], completionHandler: nil)
        }.resume()
    }
}
