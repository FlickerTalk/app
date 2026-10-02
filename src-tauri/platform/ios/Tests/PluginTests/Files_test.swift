import AVFoundation
import UniformTypeIdentifiers
import XCTest
@testable import tauri_plugin_ft_platform

// Files the user picks, photographs, saves or shares on the iPhone (2026-09-30): the same contract
// as Android's `pickFiles`, `takePhoto`, `saveToDownloads` and `shareFile`, so the rest of the
// flow (sending a picked file, `picked_path` in Rust) is shared.
final class FilesTests: XCTestCase {
    // What is picked or photographed waits in `<app data>/files/uploads`, the folder Rust accepts
    // (`picked_path`); Tauri's app data folder on iOS is Application Support/<bundle id>.
    func testPickedFilesWaitWhereRustLooks() {
        let support = URL(fileURLWithPath: "/var/mobile/Containers/Data/Application/X/Library/Application Support")
        XCTAssertEqual(
            pickedFolder(applicationSupport: support, bundle: "com.flickertalk.app").path,
            "/var/mobile/Containers/Data/Application/X/Library/Application Support/com.flickertalk.app/files/uploads"
        )
    }

    // Named by when it was taken, exactly as Android names it (`photoName` in PlatformPlugin.kt).
    func testAPhotoIsNamedByWhenItWasTaken() {
        let taken = Date(timeIntervalSince1970: 1_790_195_730) // 2026-09-23 20:35:30 UTC
        XCTAssertEqual(photoName(taken, zone: TimeZone(identifier: "UTC")!), "photo-20260923-203530.jpg")
        XCTAssertEqual(photoName(taken, zone: TimeZone(identifier: "Europe/Madrid")!), "photo-20260923-223530.jpg")
    }

    // A photo is kept only if the camera came back with one that has bytes.
    func testOnlyARealPhotoIsKept() {
        XCTAssertTrue(keepsPhoto(Data([0xFF, 0xD8, 0xFF])))
        XCTAssertFalse(keepsPhoto(Data()))
        XCTAssertFalse(keepsPhoto(nil))
    }

    // Something with no name is still a file, and a name never makes a path.
    func testPickedNamesAreSafe() {
        XCTAssertEqual(pickedName("  holidays.jpg "), "holidays.jpg")
        XCTAssertEqual(pickedName(""), "file")
        XCTAssertEqual(pickedName(nil), "file")
        XCTAssertEqual(pickedName("../../a/b.pdf"), ".._.._a_b.pdf")
    }

    // Two files with the same name picked in the same moment never overwrite each other.
    func testEachPickedFileGetsItsOwnPlace() {
        let folder = URL(fileURLWithPath: "/tmp/uploads")
        let now = Date(timeIntervalSince1970: 1_790_195_730.123)
        XCTAssertEqual(pickedTarget(folder: folder, name: "a.jpg", now: now, index: 0).lastPathComponent, "1790195730123-0-a.jpg")
        XCTAssertEqual(pickedTarget(folder: folder, name: "a.jpg", now: now, index: 1).lastPathComponent, "1790195730123-1-a.jpg")
    }

    // 2026-10-02: a photo is named by the second it was taken; a second one in that second is a
    // file of its own and never writes over the first, which a message may point to (as on
    // Android, `newPickedFile`).
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

    // What a picked file is, as far as its extension tells.
    func testWhatAPickedFileIs() {
        XCTAssertEqual(pickedMime(fileExtension: "jpg"), "image/jpeg")
        XCTAssertEqual(pickedMime(fileExtension: "pdf"), "application/pdf")
        XCTAssertEqual(pickedMime(fileExtension: "mov"), "video/quicktime")
        XCTAssertEqual(pickedMime(fileExtension: ""), "application/octet-stream")
        XCTAssertEqual(pickedMime(fileExtension: "zzqq"), "application/octet-stream")
    }

    // Pictures and videos come through the photo sheet over the chat (PHPicker, no permission);
    // anything else through the document picker, as on Android.
    func testPhotosAndVideosUseThePhotoSheet() {
        XCTAssertEqual(photoKind("image/*,video/*"), .both)
        XCTAssertEqual(photoKind("image/*"), .images)
        XCTAssertEqual(photoKind("image/png"), .images)
        XCTAssertEqual(photoKind("video/*"), .videos)
        XCTAssertNil(photoKind(""))
        XCTAssertNil(photoKind("application/pdf"))
    }

    // What the document picker is told to show; anything not understood means everything.
    func testTheDocumentPickerShowsWhatWasAskedFor() {
        XCTAssertEqual(documentTypes(""), [UTType.item])
        XCTAssertEqual(documentTypes("application/pdf"), [UTType.pdf])
        XCTAssertEqual(documentTypes("not a type"), [UTType.item])
    }

    // The camera opens when it is there and allowed; iOS asks once; otherwise the command fails
    // with a message, never hangs (the simulator has no camera).
    func testTheCameraOpensOnlyWhenItCan() {
        XCTAssertEqual(cameraStep(available: true, status: .authorized), .open)
        XCTAssertEqual(cameraStep(available: true, status: .notDetermined), .ask)
        XCTAssertEqual(cameraStep(available: true, status: .denied), .refuse("the camera is not allowed"))
        XCTAssertEqual(cameraStep(available: true, status: .restricted), .refuse("the camera is not allowed"))
        XCTAssertEqual(cameraStep(available: false, status: .authorized), .refuse("no camera on this device"))
    }

    // Saving and sharing hand over a copy named as the user knows the file, not as it is kept.
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

    // §94, 2026-09-30: erasing the phone deletes the storage key from the Keychain, which outlives
    // the app, even its removal. Forgetting a key that is not there is not an error.
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
