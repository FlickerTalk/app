import AVFoundation
import Foundation
import PhotosUI
import Tauri
import UIKit
import UniformTypeIdentifiers

// ---- Files the user picks, photographs, saves or shares (2026-09-30) ----
// The same contract as Android's `pickFiles`, `takePhoto`, `saveToDownloads` and `shareFile`
// (PlatformPlugin.kt): what is picked or photographed is copied into the app's folder and handed
// back as `{files: [{path, name, mime, size}]}`, so sending it (`core_send_picked`, `picked_path`
// in Rust) is the same on both phones.

/// Where what the user picks or photographs waits: `<app data>/files/uploads`, one of the folders
/// Rust accepts a picked path from. Tauri's app data folder on iOS is Application Support/<bundle>.
func pickedFolder(applicationSupport: URL, bundle: String) -> URL {
    applicationSupport
        .appendingPathComponent(bundle, isDirectory: true)
        .appendingPathComponent("files", isDirectory: true)
        .appendingPathComponent("uploads", isDirectory: true)
}

/// This app's own `pickedFolder`.
func appPickedFolder() throws -> URL {
    let support = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
    let folder = pickedFolder(applicationSupport: support, bundle: Bundle.main.bundleIdentifier ?? "com.flickertalk.app")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    return folder
}

/// A photo taken with the camera is named by when it was taken, like on Android.
func photoName(_ now: Date, zone: TimeZone = .current) -> String {
    let format = DateFormatter()
    format.locale = Locale(identifier: "en_US_POSIX")
    format.calendar = Calendar(identifier: .gregorian)
    format.timeZone = zone
    format.dateFormat = "'photo-'yyyyMMdd-HHmmss'.jpg'"
    return format.string(from: now)
}

/// A photo is kept only if the camera came back with one that has bytes.
func keepsPhoto(_ data: Data?) -> Bool {
    guard let data else { return false }
    return !data.isEmpty
}

/// The name to show for a picked file: something with no name is still a file, and a name never
/// makes a path.
func pickedName(_ name: String?) -> String {
    let trimmed = (name ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? "file" : trimmed.replacingOccurrences(of: "/", with: "_")
}

/// Where one picked file is copied: its own place, even for two with the same name at once.
func pickedTarget(folder: URL, name: String, now: Date, index: Int) -> URL {
    let millis = Int64((now.timeIntervalSince1970 * 1000).rounded(.down))
    return folder.appendingPathComponent("\(millis)-\(index)-\(pickedName(name))")
}

/// Where a new file named `name` goes in `folder`: `name`, or, if that is taken (two photos in
/// one second, a clock set back), `name` with `-2`, `-3`… before its extension. Never a file that
/// is there already, which a message may point to (2026-10-02, as Android's `newPickedFile`).
func freshTarget(folder: URL, name: String) -> URL {
    let stem = (name as NSString).deletingPathExtension
    let suffix = (name as NSString).pathExtension
    var attempt = 1
    while true {
        let candidate = attempt == 1 ? name : (suffix.isEmpty ? "\(stem)-\(attempt)" : "\(stem)-\(attempt).\(suffix)")
        let target = folder.appendingPathComponent(candidate)
        if !FileManager.default.fileExists(atPath: target.path) {
            return target
        }
        attempt += 1
    }
}

/// What a picked file is, as far as its extension tells.
func pickedMime(fileExtension: String) -> String {
    guard !fileExtension.isEmpty, let type = UTType(filenameExtension: fileExtension), let mime = type.preferredMIMEType else {
        return "application/octet-stream"
    }
    return mime
}

/// What the photo sheet shows.
enum PhotoKind: Equatable {
    case images, videos, both
}

/// Pictures and videos come through the photo sheet over the chat (no permission needed); `nil`
/// means anything else, through the document picker.
func photoKind(_ accept: String) -> PhotoKind? {
    let parts = accept.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
    guard !parts.isEmpty else { return nil }
    let images = parts.allSatisfy { $0.hasPrefix("image/") || $0.hasPrefix("video/") } && parts.contains { $0.hasPrefix("image/") }
    let videos = parts.allSatisfy { $0.hasPrefix("image/") || $0.hasPrefix("video/") } && parts.contains { $0.hasPrefix("video/") }
    switch (images, videos) {
    case (true, true): return .both
    case (true, false): return .images
    case (false, true): return .videos
    default: return nil
    }
}

/// What the document picker is told to show. Anything not understood means everything.
func documentTypes(_ accept: String) -> [UTType] {
    let types = accept.split(separator: ",").compactMap { UTType(mimeType: $0.trimmingCharacters(in: .whitespaces)) }
    return types.isEmpty ? [.item] : types
}

/// What `takePhoto` does next.
enum CameraStep: Equatable {
    case open
    /// iOS asks the user once.
    case ask
    /// Fails with this message: never hangs (the simulator has no camera).
    case refuse(String)
}

func cameraStep(available: Bool, status: AVAuthorizationStatus) -> CameraStep {
    guard available else { return .refuse("no camera on this device") }
    switch status {
    case .authorized: return .open
    case .notDetermined: return .ask
    default: return .refuse("the camera is not allowed")
    }
}

/// A copy of a file of the app named as the user knows it, in a folder of its own under the
/// temporary folder: saving and sharing hand this over, never the file as the app keeps it.
func namedCopy(of path: String, name: String) throws -> URL {
    let source = URL(fileURLWithPath: path)
    let folder = FileManager.default.temporaryDirectory.appendingPathComponent("ft-out-\(UUID().uuidString)", isDirectory: true)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let target = folder.appendingPathComponent(pickedName(name))
    do {
        try FileManager.default.copyItem(at: source, to: target)
    } catch {
        try? FileManager.default.removeItem(at: folder)
        throw error
    }
    return target
}

/// One picked file as Rust reads it (`PickedFile` in ../../src/lib.rs).
func pickedEntry(_ url: URL, name: String) -> [String: Any] {
    let size = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? NSNumber)?.int64Value ?? 0
    return ["path": url.path, "name": name, "mime": pickedMime(fileExtension: url.pathExtension), "size": size]
}

/// Deletes the storage key from the Keychain (erasing the phone, 2026-09-30): the Keychain
/// outlives the app, even its removal. A key that is not there is already forgotten.
func forgetStoredKey() throws {
    let status = SecItemDelete(keyQuery() as CFDictionary)
    guard status == errSecSuccess || status == errSecItemNotFound else { throw KeychainError.status(status) }
}

struct PickArgs: Decodable {
    let accept: String?
}

/// Arguments of `saveToDownloads` and `shareFile`, as Android reads them (`SaveFileArgs`).
struct SaveFileArgs: Decodable {
    let path: String
    let name: String
    let mime: String
}

/// Keeps a delegate alive as long as the screen it serves is shown.
private func keep(_ delegate: AnyObject, with controller: UIViewController) {
    objc_setAssociatedObject(controller, "ft.files.delegate", delegate, .OBJC_ASSOCIATION_RETAIN)
}

/// The camera: one photo, as JPEG (without the camera's metadata, so no place), into the app's
/// folder. Backing out hands back nothing.
final class PhotoTaker: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
    private let invoke: Invoke

    init(_ invoke: Invoke) {
        self.invoke = invoke
    }

    func imagePickerController(_ picker: UIImagePickerController, didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]) {
        let data = (info[.originalImage] as? UIImage)?.jpegData(compressionQuality: 0.9)
        picker.dismiss(animated: true)
        guard keepsPhoto(data), let data else {
            invoke.resolve(["files": []])
            return
        }
        do {
            let target = freshTarget(folder: try appPickedFolder(), name: photoName(Date()))
            // Never over a file that is there: it fails instead, and the user takes it again.
            try data.write(to: target, options: [.withoutOverwriting, .completeFileProtectionUntilFirstUserAuthentication])
            invoke.resolve(["files": [pickedEntry(target, name: target.lastPathComponent)]])
        } catch {
            invoke.reject("cannot keep the photo: \(error.localizedDescription)")
        }
    }

    func imagePickerControllerDidCancel(_ picker: UIImagePickerController) {
        picker.dismiss(animated: true)
        invoke.resolve(["files": []])
    }
}

/// The photo sheet: pictures and videos in their most compatible form (JPEG, H.264), so the
/// contact's phone can show them whatever it is. It needs no permission: the app sees only what
/// the user picked.
final class PhotoPicker: NSObject, PHPickerViewControllerDelegate {
    private let invoke: Invoke

    init(_ invoke: Invoke) {
        self.invoke = invoke
    }

    func picker(_ picker: PHPickerViewController, didFinishPicking results: [PHPickerResult]) {
        picker.dismiss(animated: true)
        let folder: URL
        do {
            folder = try appPickedFolder()
        } catch {
            invoke.reject("cannot keep what was picked: \(error.localizedDescription)")
            return
        }
        let group = DispatchGroup()
        let lock = NSLock()
        var picked: [(Int, [String: Any])] = []
        let now = Date()
        for (index, result) in results.enumerated() {
            let provider = result.itemProvider
            guard let type = provider.registeredTypeIdentifiers.first(where: { identifier in
                UTType(identifier).map { $0.conforms(to: .image) || $0.conforms(to: .movie) } ?? false
            }) else { continue }
            group.enter()
            provider.loadFileRepresentation(forTypeIdentifier: type) { url, _ in
                defer { group.leave() }
                guard let url else { return }
                // The file only lives until this returns: it is copied right here.
                let base = provider.suggestedName.map { pickedName($0) } ?? "file"
                let name = url.pathExtension.isEmpty ? base : "\(base).\(url.pathExtension.lowercased())"
                let target = pickedTarget(folder: folder, name: name, now: now, index: index)
                guard (try? FileManager.default.copyItem(at: url, to: target)) != nil else { return }
                lock.lock()
                picked.append((index, pickedEntry(target, name: name)))
                lock.unlock()
            }
        }
        group.notify(queue: .main) { [invoke] in
            invoke.resolve(["files": picked.sorted { $0.0 < $1.0 }.map(\.1)])
        }
    }
}

/// The document picker: copies of what the user chose, moved into the app's folder.
final class DocumentPicker: NSObject, UIDocumentPickerDelegate {
    private let invoke: Invoke

    init(_ invoke: Invoke) {
        self.invoke = invoke
    }

    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        do {
            let folder = try appPickedFolder()
            let now = Date()
            var picked: [[String: Any]] = []
            for (index, url) in urls.enumerated() {
                let name = pickedName(url.lastPathComponent)
                let target = pickedTarget(folder: folder, name: name, now: now, index: index)
                try FileManager.default.moveItem(at: url, to: target)
                picked.append(pickedEntry(target, name: name))
            }
            invoke.resolve(["files": picked])
        } catch {
            invoke.reject("cannot read what was picked: \(error.localizedDescription)")
        }
    }

    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        invoke.resolve(["files": []])
    }
}

/// "Save to Files": the user picks where. It answers once the copy is there, and fails if the
/// user backs out, so nothing is said to be saved that is not.
final class FileSaver: NSObject, UIDocumentPickerDelegate {
    private let invoke: Invoke
    private let copy: URL

    init(_ invoke: Invoke, copy: URL) {
        self.invoke = invoke
        self.copy = copy
    }

    private func done() {
        try? FileManager.default.removeItem(at: copy.deletingLastPathComponent())
    }

    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        done()
        invoke.resolve()
    }

    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        done()
        invoke.reject("cancelled")
    }
}

extension PlatformPlugin {
    /// Picks files and copies them into the app's folder (§62): pictures and videos through the
    /// photo sheet, anything else through the document picker. Backing out hands back nothing.
    @objc public func pickFiles(_ invoke: Invoke) throws {
        let accept = (try? invoke.parseArgs(PickArgs.self))?.accept ?? ""
        DispatchQueue.main.async { [manager] in
            guard let screen = manager.viewController else {
                invoke.reject("no screen to pick from")
                return
            }
            let controller: UIViewController
            if let kind = photoKind(accept) {
                var configuration = PHPickerConfiguration()
                configuration.selectionLimit = 20
                configuration.preferredAssetRepresentationMode = .compatible
                switch kind {
                case .images: configuration.filter = .images
                case .videos: configuration.filter = .videos
                case .both: configuration.filter = .any(of: [.images, .videos])
                }
                let picker = PHPickerViewController(configuration: configuration)
                let delegate = PhotoPicker(invoke)
                picker.delegate = delegate
                keep(delegate, with: picker)
                controller = picker
            } else {
                let picker = UIDocumentPickerViewController(forOpeningContentTypes: documentTypes(accept), asCopy: true)
                picker.allowsMultipleSelection = true
                let delegate = DocumentPicker(invoke)
                picker.delegate = delegate
                keep(delegate, with: picker)
                controller = picker
            }
            screen.present(controller, animated: true)
        }
    }

    /// A photo taken now with the camera, into the app's folder like a picked file (Android's
    /// `takePhoto`). iOS asks for the camera once, and the camera opens as soon as it is allowed.
    /// With no camera (the simulator) or none allowed, it fails with a message.
    @objc public func takePhoto(_ invoke: Invoke) throws {
        let available = UIImagePickerController.isSourceTypeAvailable(.camera)
        switch cameraStep(available: available, status: AVCaptureDevice.authorizationStatus(for: .video)) {
        case .refuse(let reason):
            invoke.reject(reason)
        case .ask:
            AVCaptureDevice.requestAccess(for: .video) { [weak self] granted in
                guard granted else {
                    invoke.reject("the camera is not allowed")
                    return
                }
                self?.openCamera(invoke)
            }
        case .open:
            openCamera(invoke)
        }
    }

    private func openCamera(_ invoke: Invoke) {
        DispatchQueue.main.async { [manager] in
            guard let screen = manager.viewController else {
                invoke.reject("no screen to take a photo from")
                return
            }
            let camera = UIImagePickerController()
            camera.sourceType = .camera
            camera.mediaTypes = [UTType.image.identifier]
            let delegate = PhotoTaker(invoke)
            camera.delegate = delegate
            keep(delegate, with: camera)
            screen.present(camera, animated: true)
        }
    }

    /// Android copies a file to Downloads; an iPhone has no such folder, so the system's "Save to
    /// Files" sheet lets the user pick where (Downloads among them).
    @objc public func saveToDownloads(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(SaveFileArgs.self)
        let copy: URL
        do {
            copy = try namedCopy(of: args.path, name: args.name)
        } catch {
            invoke.reject("there is nothing to save")
            return
        }
        DispatchQueue.main.async { [manager] in
            guard let screen = manager.viewController else {
                try? FileManager.default.removeItem(at: copy.deletingLastPathComponent())
                invoke.reject("no screen to save from")
                return
            }
            let picker = UIDocumentPickerViewController(forExporting: [copy], asCopy: true)
            let delegate = FileSaver(invoke, copy: copy)
            picker.delegate = delegate
            keep(delegate, with: picker)
            screen.present(picker, animated: true)
        }
    }

    /// Hands a file of the app to another app through the share sheet ("Save Image" is there
    /// too), named as the user knows it.
    @objc public func shareFile(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(SaveFileArgs.self)
        let copy: URL
        do {
            copy = try namedCopy(of: args.path, name: args.name)
        } catch {
            invoke.reject("there is nothing to share")
            return
        }
        DispatchQueue.main.async { [manager] in
            guard let screen = manager.viewController else {
                try? FileManager.default.removeItem(at: copy.deletingLastPathComponent())
                invoke.reject("no screen to share from")
                return
            }
            let sheet = UIActivityViewController(activityItems: [copy], applicationActivities: nil)
            sheet.completionWithItemsHandler = { _, _, _, _ in
                try? FileManager.default.removeItem(at: copy.deletingLastPathComponent())
            }
            // On an iPad the sheet is a popover: it needs an anchor.
            UIUtils.centerPopover(rootViewController: screen, popoverController: sheet)
            screen.present(sheet, animated: true)
            invoke.resolve()
        }
    }

    /// Erasing the phone (2026-09-30): the storage key leaves the Keychain.
    @objc public func forgetKey(_ invoke: Invoke) throws {
        do {
            try forgetStoredKey()
            invoke.resolve()
        } catch {
            invoke.reject("the Keychain kept the key: \(error)")
        }
    }
}
