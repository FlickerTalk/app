import XCTest
@testable import tauri_plugin_ft_platform

final class AuthorizeTests: XCTestCase {
    func testOnlyOurSchemeEndsALogin() throws {
        XCTAssertTrue(isAuthRedirect(URL(string: "com.flickertalk.app:/oauth?code=abc"), scheme: "com.flickertalk.app"))
        XCTAssertFalse(isAuthRedirect(URL(string: "https://evil.example/?code=abc"), scheme: "com.flickertalk.app"))
        XCTAssertFalse(isAuthRedirect(nil, scheme: "com.flickertalk.app"))
    }
}
