import Foundation
import StoreKit
import Tauri
import UIKit

// product id in App Store Connect
let yearly = "yearly"

struct StoreEntitlement {
    let product: String
    let expires: Date?
    let revoked: Date?
}

// ms
func activeUntil(_ entitlements: [StoreEntitlement], now: Date) -> Int64 {
    entitlements
        .filter { $0.product == yearly && $0.revoked == nil }
        .compactMap { $0.expires }
        .filter { $0 > now }
        .map { Int64(($0.timeIntervalSince1970 * 1000).rounded()) }
        .max() ?? 0
}

struct StoreProduct {
    let id: String
    let displayPrice: String
}

func yearlyPrice(_ products: [StoreProduct]) -> String? {
    guard let price = products.first(where: { $0.id == yearly })?.displayPrice
        .trimmingCharacters(in: .whitespacesAndNewlines), !price.isEmpty else { return nil }
    return price
}

func restoreTrouble(_ error: Error) -> String {
    if case StoreKitError.userCancelled = error { return "cancelled" }
    return "store_unavailable"
}

final class EntitlementQueue {
    private var sink: ((Int64) -> Void)?
    private var latest: Int64?

    func register(_ sink: @escaping (Int64) -> Void) {
        self.sink = sink
        if let latest {
            self.latest = nil
            sink(latest)
        }
    }

    func offer(_ until: Int64) {
        if let sink {
            sink(until)
        } else {
            latest = until
        }
    }
}

func currentEntitlements() async -> [StoreEntitlement] {
    var found: [StoreEntitlement] = []
    for await result in Transaction.currentEntitlements {
        guard case .verified(let transaction) = result else { continue }
        found.append(
            StoreEntitlement(
                product: transaction.productID,
                expires: transaction.expirationDate,
                revoked: transaction.revocationDate
            )
        )
    }
    return found
}

// Apple wants transaction updates listened to from launch, or transactions stay unfinished
final class StoreUpdates {
    static let shared = StoreUpdates()
    private let queue = DispatchQueue(label: "com.flickertalk.store.updates")
    private let entitlements = EntitlementQueue()
    private var started = false
    private var observer: NSObjectProtocol?

    func register(_ channel: Channel) {
        queue.async { self.entitlements.register { channel.send(["until": $0] as JsonObject) } }
    }

    func start() {
        queue.sync {
            guard !started else { return }
            started = true
        }
        Task.detached {
            for await result in Transaction.updates {
                // only finish what Apple signed, then reread the entitlements either way
                if case .verified(let transaction) = result { await transaction.finish() }
                await StoreUpdates.shared.readNow()
            }
        }
        // an expiry is no transaction, it only shows in the subscription status (iOS 17)
        if #available(iOS 17.0, *) {
            Task.detached {
                for await _ in Product.SubscriptionInfo.Status.updates {
                    await StoreUpdates.shared.readNow()
                }
            }
        }
        DispatchQueue.main.async {
            self.observer = NotificationCenter.default.addObserver(
                forName: UIApplication.didBecomeActiveNotification, object: nil, queue: .main
            ) { _ in
                Task { await StoreUpdates.shared.readNow() }
            }
        }
    }

    func readNow() async {
        let until = activeUntil(await currentEntitlements(), now: Date())
        queue.async { self.entitlements.offer(until) }
    }
}

extension PlatformPlugin {
    // rejections are plan.trouble.* keys, the app translates them
    private func entitlements() async -> [StoreEntitlement] {
        await currentEntitlements()
    }

    @objc public func registerEntitlementEvents(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(CallEventsArgs.self)
        StoreUpdates.shared.register(args.channel)
        invoke.resolve()
    }

    @objc public func subscription(_ invoke: Invoke) throws {
        Task {
            invoke.resolve(["until": activeUntil(await entitlements(), now: Date())])
        }
    }

    @objc public func restoreSubscription(_ invoke: Invoke) throws {
        Task {
            do {
                try await AppStore.sync()
                invoke.resolve(["until": activeUntil(await entitlements(), now: Date())])
            } catch {
                invoke.reject(restoreTrouble(error))
            }
        }
    }

    @objc public func subscriptionPrice(_ invoke: Invoke) throws {
        Task {
            let products = (try? await Product.products(for: [yearly])) ?? []
            let price = yearlyPrice(products.map { StoreProduct(id: $0.id, displayPrice: $0.displayPrice) })
            if let price {
                invoke.resolve(["price": price])
            } else {
                invoke.resolve()
            }
        }
    }

    @objc public func subscribe(_ invoke: Invoke) throws {
        Task {
            do {
                guard let product = try await Product.products(for: [yearly]).first else {
                    invoke.reject("not_on_sale")
                    return
                }
                switch try await product.purchase() {
                case .success(let signed):
                    guard case .verified(let transaction) = signed else {
                        invoke.reject("payment_failed")
                        return
                    }
                    await transaction.finish()
                    invoke.resolve(["until": activeUntil(await entitlements(), now: Date())])
                case .userCancelled:
                    invoke.reject("cancelled")
                case .pending:
                    // Ask to Buy isn't paid for yet, don't pretend it is
                    invoke.reject("pending_approval")
                @unknown default:
                    invoke.reject("payment_failed")
                }
            } catch {
                invoke.reject("payment_failed")
            }
        }
    }
}
