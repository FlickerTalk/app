import XCTest
@testable import tauri_plugin_ft_platform

// Links that open the app (Universal Links, 2026-10-08): iOS hands the link over as a web
// browsing user activity, when the scene connects (a cold start) or continues (the app ran). The
// bridge keeps it until the app asks for it, once; Rust says which page it leads to.
final class OpenedLinksTests: XCTestCase {
    private let link = "https://flickertalk.com/add#card"

    func testAWebBrowsingActivityOpensItsLink() {
        XCTAssertEqual(openedLinkOf(activityType: NSUserActivityTypeBrowsingWeb, url: URL(string: link)), link)
        XCTAssertEqual(
            openedLinkOf(activityType: NSUserActivityTypeBrowsingWeb, url: URL(string: "https://flickertalk.com/move#invite")),
            "https://flickertalk.com/move#invite")
    }

    // CallKit's call intents come the same way: they open no link.
    func testOtherActivitiesOpenNoLink() {
        XCTAssertNil(openedLinkOf(activityType: "INStartCallIntent", url: URL(string: link)))
        XCTAssertNil(openedLinkOf(activityType: "INStartVideoCallIntent", url: nil))
        XCTAssertNil(openedLinkOf(activityType: NSUserActivityTypeBrowsingWeb, url: nil))
        XCTAssertNil(openedLinkOf(activityType: NSUserActivityTypeBrowsingWeb, url: URL(string: "flickertalk://add#card")))
        XCTAssertNil(openedLinkOf(activityType: NSUserActivityTypeBrowsingWeb, url: URL(string: "http://flickertalk.com/add#card")))
    }

    func testTheLinkIsHandedOverOnceAndTheLatestWins() {
        let links = OpenedLinks()
        XCTAssertEqual(links.take(), "")
        links.put(link)
        links.put("https://flickertalk.com/move#invite")
        XCTAssertEqual(links.take(), "https://flickertalk.com/move#invite")
        XCTAssertEqual(links.take(), "", "taken already")
    }

    func testAListenerHearsEachLinkAsItComes() {
        let links = OpenedLinks()
        var heard = 0
        links.put(link)
        links.register { heard += 1 }
        XCTAssertEqual(heard, 0, "what came before the listener is asked for, not told")
        links.put(link)
        XCTAssertEqual(heard, 1)
        links.register { heard += 10 }
        links.put(link)
        XCTAssertEqual(heard, 11, "a second listener replaces the first")
        XCTAssertEqual(links.take(), link)
    }
}
