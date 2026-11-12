import StoreKit
import XCTest
@testable import tauri_plugin_ft_platform

final class BillingTests: XCTestCase {
    private func entitlement(
        product: String = yearly,
        expires: Date? = Date(timeIntervalSince1970: 1_821_744_000),
        revoked: Date? = nil
    ) -> StoreEntitlement {
        StoreEntitlement(product: product, expires: expires, revoked: revoked)
    }

    func testTheStoreOwnDateIsKept() throws {
        let runsOut = Date(timeIntervalSince1970: 1_821_744_000)
        let now = Date(timeIntervalSince1970: 1_790_208_000)
        XCTAssertEqual(activeUntil([entitlement(expires: runsOut)], now: now), 1_821_744_000_000)
    }

    func testNothingBoughtIsNoSubscription() throws {
        XCTAssertEqual(activeUntil([], now: Date(timeIntervalSince1970: 1_790_208_000)), 0)
    }

    func testOnlyTheYearlyProductCounts() throws {
        let now = Date(timeIntervalSince1970: 1_790_208_000)
        XCTAssertEqual(activeUntil([entitlement(product: "com.someone.else.pro")], now: now), 0)
    }

    func testARefundedPurchaseStopsCounting() throws {
        let now = Date(timeIntervalSince1970: 1_790_208_000)
        let refunded = entitlement(revoked: Date(timeIntervalSince1970: 1_790_100_000))
        XCTAssertEqual(activeUntil([refunded], now: now), 0)
    }

    func testAnEntitlementThatRanOutIsNotASubscription() throws {
        let now = Date(timeIntervalSince1970: 1_790_208_000)
        let old = entitlement(expires: Date(timeIntervalSince1970: 1_790_100_000))
        XCTAssertEqual(activeUntil([old], now: now), 0)
    }

    func testTheFurthestEntitlementCounts() throws {
        let now = Date(timeIntervalSince1970: 1_790_208_000)
        let soon = entitlement(expires: Date(timeIntervalSince1970: 1_800_000_000))
        let later = entitlement(expires: Date(timeIntervalSince1970: 1_821_744_000))
        XCTAssertEqual(activeUntil([soon, later], now: now), 1_821_744_000_000)
    }

    func testTheYearlyPriceIsAsTheStoreFormatsIt() {
        XCTAssertEqual(yearlyPrice([StoreProduct(id: yearly, displayPrice: "0,99 €")]), "0,99 €")
        XCTAssertEqual(yearlyPrice([StoreProduct(id: "com.someone.else.pro", displayPrice: "9,99 €"), StoreProduct(id: yearly, displayPrice: "$0.99")]), "$0.99")
    }

    func testBackingOutOfTheAppleIdSignInWhileRestoringIsNoTrouble() {
        XCTAssertEqual(restoreTrouble(StoreKitError.userCancelled), "cancelled")
        XCTAssertEqual(restoreTrouble(StoreKitError.networkError(URLError(.notConnectedToInternet))), "store_unavailable")
        XCTAssertEqual(restoreTrouble(URLError(.timedOut)), "store_unavailable")
    }

    func testALiveEntitlementWaitsForTheCoreAndOnlyTheLatestCounts() {
        let queue = EntitlementQueue()
        queue.offer(1_800_000_000_000)
        queue.offer(0)
        var heard: [Int64] = []
        queue.register { heard.append($0) }
        XCTAssertEqual(heard, [0])
        queue.offer(1_830_000_000_000)
        XCTAssertEqual(heard, [0, 1_830_000_000_000])
        var later: [Int64] = []
        queue.register { later.append($0) }
        XCTAssertEqual(later, [])
        queue.offer(5)
        XCTAssertEqual(heard, [0, 1_830_000_000_000])
        XCTAssertEqual(later, [5])
    }

    func testWithoutTheYearlyProductThereIsNoPrice() {
        XCTAssertNil(yearlyPrice([]))
        XCTAssertNil(yearlyPrice([StoreProduct(id: "com.someone.else.pro", displayPrice: "9,99 €")]))
        XCTAssertNil(yearlyPrice([StoreProduct(id: yearly, displayPrice: "  ")]))
    }
}
