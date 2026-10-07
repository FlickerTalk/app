import StoreKit
import StoreKitTest
import XCTest

/// Drives FlickerTalk's subscription against `FlickerTalk.storekit` in the simulator.
///
/// `ACTIONS` (passed as `TEST_RUNNER_ACTIONS`) is a list separated by `;`:
///   session           load the configuration for com.flickertalk.app (keeps what was bought)
///   reset             forget every test transaction of the app
///   expire            make the yearly subscription run out now
///   refund            refund the newest yearly transaction
///   fail:<on|off>     make the next purchases fail (StoreKit error) or succeed again
///   asktobuy:<on|off> purchases wait for a parent's approval (Ask to Buy)
///   approve           approve every purchase waiting for it
///   list              print the app's test transactions
///   button:<text>     tap the first button whose label contains the text (app or system sheet)
///   dump              print the texts and buttons on the screen
///   tap:x,y           tap a point of the screen (points from the top-left)
///   wait:<seconds>
final class Driver: XCTestCase {
    static let app = "com.flickertalk.app"

    @MainActor
    func testRun() async throws {
        let actions = (ProcessInfo.processInfo.environment["ACTIONS"] ?? "")
            .split(separator: ";").map(String.init)
        let board = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let flicker = XCUIApplication(bundleIdentifier: Self.app)
        var session: SKTestSession?

        func open() throws -> SKTestSession {
            if let session { return session }
            let url = try XCTUnwrap(Bundle(for: Driver.self).url(forResource: "FlickerTalk", withExtension: "storekit"))
            let opened = try SKTestSession(contentsOf: url)
            // A UI-test runner's session belongs to the runner itself, not to the app it drives,
            // and StoreKitTest has no public way to name another app. Its own `bundleID` and
            // `_configureWithURL:error:` (what the initializer calls) do it; both are private, which
            // is fine for a test tool that never ships.
            opened.setValue(Self.app, forKey: "bundleID")
            _ = opened.perform(NSSelectorFromString("_configureWithURL:error:"), with: url, with: nil)
            print("DRIVER: session for \(opened.value(forKey: "bundleID") ?? "?")")
            opened.disableDialogs = false
            session = opened
            return opened
        }

        for action in actions {
            let parts = action.split(separator: ":", maxSplits: 1).map(String.init)
            let verb = parts[0]
            let arg = parts.count > 1 ? parts[1] : ""
            switch verb {
            case "session":
                _ = try open()
            case "reset":
                try open().clearTransactions()
                print("DRIVER: transactions cleared")
            case "expire":
                try open().expireSubscription(productIdentifier: "yearly")
                print("DRIVER: yearly expired")
            case "refund":
                let session = try open()
                if let newest = session.allTransactions().filter({ $0.productIdentifier == "yearly" })
                    .max(by: { $0.purchaseDate < $1.purchaseDate }) {
                    try session.refundTransaction(identifier: newest.identifier)
                    print("DRIVER: refunded \(newest.identifier)")
                }
            case "fail":
                // A purchase the Store refuses (a declined card, say): StoreKitError.unknown.
                let error: SKTestFailures.Purchase? = arg == "on" ? .generic(.unknown) : nil
                try await open().setSimulatedError(error, forAPI: .purchase)
                print("DRIVER: purchase failure \(arg)")
            case "asktobuy":
                try open().askToBuyEnabled = (arg == "on")
                print("DRIVER: ask to buy \(arg)")
            case "approve":
                let session = try open()
                for one in session.allTransactions() where one.pendingAskToBuyConfirmation {
                    try session.approveAskToBuyTransaction(identifier: one.identifier)
                    print("DRIVER: approved \(one.identifier)")
                }
            case "list":
                for one in try open().allTransactions() {
                    print("DRIVER: tx \(one.identifier) \(one.productIdentifier) state=\(one.state.rawValue) "
                        + "purchased=\(one.purchaseDate) expires=\(String(describing: one.expirationDate)) "
                        + "cancelled=\(String(describing: one.cancelDate)) renews=\(one.autoRenewingEnabled)")
                }
            case "button":
                // The first button whose label contains the text, in a system sheet or in the app
                // (the WebView's buttons are accessibility elements too).
                let match = NSPredicate(format: "label CONTAINS[c] %@", arg)
                var found: XCUIElement?
                for _ in 0..<16 where found == nil {
                    found = [board, flicker].map { $0.buttons.matching(match).firstMatch }.first { $0.exists }
                    if found == nil { try await Task.sleep(for: .milliseconds(500)) }
                }
                if let found { found.tap() } else { print("DRIVER: no button \(arg)") }
            case "dump":
                // What is on the screen, as accessibility sees it: the evidence of a step.
                let texts = (flicker.staticTexts.allElementsBoundByIndex + board.staticTexts.allElementsBoundByIndex)
                    .map(\.label).filter { !$0.isEmpty }
                let buttons = (flicker.buttons.allElementsBoundByIndex + board.buttons.allElementsBoundByIndex)
                    .map(\.label).filter { !$0.isEmpty }
                print("DRIVER: texts \(texts)")
                print("DRIVER: buttons \(buttons)")
            case "tap":
                let xy = arg.split(separator: ",").compactMap { Double($0) }
                board.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: xy[0], dy: xy[1])).tap()
            case "wait":
                try await Task.sleep(for: .seconds(Double(arg) ?? 1))
            default:
                print("DRIVER: unknown \(verb)")
            }
            try await Task.sleep(for: .milliseconds(400))
        }
    }
}
