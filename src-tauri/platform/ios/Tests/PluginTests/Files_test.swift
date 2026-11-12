import AVFoundation
import UniformTypeIdentifiers
import XCTest
@testable import tauri_plugin_ft_platform

final class FilesTests: XCTestCase {
    func testPickedFilesWaitWhereRustLooks() {
        let support = URL(fileURLWithPath: "/var/mobile/Containers/Data/Application/X/Library/Application Support")
        XCTAssertEqual(
            pickedFolder(applicationSupport: support, bundle: "com.flickertalk.app").path,
            "/var/mobile/Containers/Data/Application/X/Library/Application Support/com.flickertalk.app/files/uploads"
        )
    }

    func testAPhotoIsNamedByWhenItWasTaken() {
        let taken = Date(timeIntervalSince1970: 1_790_195_730)
        XCTAssertEqual(photoName(taken, zone: TimeZone(identifier: "UTC")!), "photo-20260923-203530.jpg")
        XCTAssertEqual(photoName(taken, zone: TimeZone(identifier: "Europe/Madrid")!), "photo-20260923-223530.jpg")
    }

    func testOnlyARealPhotoIsKept() {
        XCTAssertTrue(keepsPhoto(Data([0xFF, 0xD8, 0xFF])))
        XCTAssertFalse(keepsPhoto(Data()))
        XCTAssertFalse(keepsPhoto(nil))
    }

    func testPickedNamesAreSafe() {
        XCTAssertEqual(pickedName("  holidays.jpg "), "holidays.jpg")
        XCTAssertEqual(pickedName(""), "file")
        XCTAssertEqual(pickedName(nil), "file")
        XCTAssertEqual(pickedName("../../a/b.pdf"), ".._.._a_b.pdf")
    }

    func testEachPickedFileGetsItsOwnPlace() {
        let folder = URL(fileURLWithPath: "/tmp/uploads")
        let now = Date(timeIntervalSince1970: 1_790_195_730.123)
        XCTAssertEqual(pickedTarget(folder: folder, name: "a.jpg", now: now, index: 0).lastPathComponent, "1790195730123-0-a.jpg")
        XCTAssertEqual(pickedTarget(folder: folder, name: "a.jpg", now: now, index: 1).lastPathComponent, "1790195730123-1-a.jpg")
    }

    func testASecondPhotoInTheSameSecondIsAFileOfItsOwn() throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("ft-photos-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let name = "photo-20260923-203530.jpg"

        let first = freshTarget(folder: folder, name: name)
        XCTAssertEqual(first.lastPathComponent, name)
        try Data("first".utf8).write(to: first)
        let second = freshTarget(folder: folder, name: name)
        XCTAssertEqual(second.lastPathComponent, "photo-20260923-203530-2.jpg")
        try Data("second".utf8).write(to: second)
        XCTAssertEqual(freshTarget(folder: folder, name: name).lastPathComponent, "photo-20260923-203530-3.jpg")
        XCTAssertEqual(try Data(contentsOf: first), Data("first".utf8))
        XCTAssertEqual(freshTarget(folder: folder, name: "file").lastPathComponent, "file")
    }

    func testWhatAPickedFileIs() {
        XCTAssertEqual(pickedMime(fileExtension: "jpg"), "image/jpeg")
        XCTAssertEqual(pickedMime(fileExtension: "pdf"), "application/pdf")
        XCTAssertEqual(pickedMime(fileExtension: "mov"), "video/quicktime")
        XCTAssertEqual(pickedMime(fileExtension: ""), "application/octet-stream")
        XCTAssertEqual(pickedMime(fileExtension: "zzqq"), "application/octet-stream")
    }

    func testPhotosAndVideosUseThePhotoSheet() {
        XCTAssertEqual(photoKind("image/*,video/*"), .both)
        XCTAssertEqual(photoKind("image/*"), .images)
        XCTAssertEqual(photoKind("image/png"), .images)
        XCTAssertEqual(photoKind("video/*"), .videos)
        XCTAssertNil(photoKind(""))
        XCTAssertNil(photoKind("application/pdf"))
    }

    func testTheDocumentPickerShowsWhatWasAskedFor() {
        XCTAssertEqual(documentTypes(""), [UTType.item])
        XCTAssertEqual(documentTypes("application/pdf"), [UTType.pdf])
        XCTAssertEqual(documentTypes("not a type"), [UTType.item])
    }

    func testTheCameraOpensOnlyWhenItCan() {
        XCTAssertEqual(cameraStep(available: true, status: .authorized), .open)
        XCTAssertEqual(cameraStep(available: true, status: .notDetermined), .ask)
        XCTAssertEqual(cameraStep(available: true, status: .denied), .refuse("the camera is not allowed"))
        XCTAssertEqual(cameraStep(available: true, status: .restricted), .refuse("the camera is not allowed"))
        XCTAssertEqual(cameraStep(available: false, status: .authorized), .refuse("no camera on this device"))
    }

    func testAFileLeavesWithItsOwnName() throws {
        let kept = FileManager.default.temporaryDirectory.appendingPathComponent("ft-kept-\(UUID().uuidString)")
        try Data("pdf".utf8).write(to: kept)
        addTeardownBlock { try? FileManager.default.removeItem(at: kept) }
        let named = try namedCopy(of: kept.path, name: "Invoice.pdf")
        addTeardownBlock { try? FileManager.default.removeItem(at: named.deletingLastPathComponent()) }
        XCTAssertEqual(named.lastPathComponent, "Invoice.pdf")
        XCTAssertEqual(try Data(contentsOf: named), Data("pdf".utf8))
        XCTAssertThrowsError(try namedCopy(of: "/nowhere/at/all", name: "x.pdf"))
    }

    func testErasingForgetsTheKeyInTheKeychain() throws {
        do {
            try storeKey(Data(repeating: 9, count: 32))
        } catch KeychainError.status(let status) where status == errSecMissingEntitlement {
            throw XCTSkip("this test runner has no Keychain")
        }
        XCTAssertEqual(try loadKey(), Data(repeating: 9, count: 32))
        try forgetStoredKey()
        XCTAssertThrowsError(try loadKey())
        XCTAssertNoThrow(try forgetStoredKey())
    }
}
