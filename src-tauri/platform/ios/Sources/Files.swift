import AVFoundation
import Foundation
import PhotosUI
import QuickLook
import Tauri
import UIKit
import UniformTypeIdentifiers

// must match where Rust accepts picked paths. Tauri's app data dir on iOS is Application Support/<bundle>
func pickedFolder(applicationSupport: URL, bundle: String) -> URL {
    applicationSupport
        .appendingPathComponent(bundle, isDirectory: true)
        .appendingPathComponent("files", isDirectory: true)
        .appendingPathComponent("uploads", isDirectory: true)
}

func appPickedFolder() throws -> URL {
    let support = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
    let folder = pickedFolder(applicationSupport: support, bundle: Bundle.main.bundleIdentifier ?? "com.flickertalk.app")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    return folder
}

func photoName(_ now: Date, zone: TimeZone = .current) -> String {
    let format = DateFormatter()
    format.locale = Locale(identifier: "en_US_POSIX")
    format.calendar = Calendar(identifier: .gregorian)
    format.timeZone = zone
    format.dateFormat = "'photo-'yyyyMMdd-HHmmss'.jpg'"
    return format.string(from: now)
}

func keepsPhoto(_ data: Data?) -> Bool {
    guard let data else { return false }
    return !data.isEmpty
}

// a name must never become a path
func pickedName(_ name: String?) -> String {
    let trimmed = (name ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? "file" : trimmed.replacingOccurrences(of: "/", with: "_")
}

func pickedTarget(folder: URL, name: String, now: Date, index: Int) -> URL {
    let millis = Int64((now.timeIntervalSince1970 * 1000).rounded(.down))
    return folder.appendingPathComponent("\(millis)-\(index)-\(pickedName(name))")
}

// never overwrite, a message may point to the file
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

func pickedMime(fileExtension: String) -> String {
    guard !fileExtension.isEmpty, let type = UTType(filenameExtension: fileExtension), let mime = type.preferredMIMEType else {
        return "application/octet-stream"
    }
    return mime
}

enum PhotoKind: Equatable {
    case images, videos, both
}

// nil = document picker
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

func documentTypes(_ accept: String) -> [UTType] {
    let types = accept.split(separator: ",").compactMap { UTType(mimeType: $0.trimmingCharacters(in: .whitespaces)) }
    return types.isEmpty ? [.item] : types
}

enum CameraStep: Equatable {
    case open
    case ask
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

// hand over a renamed copy, never the app's own file
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

func pickedEntry(_ url: URL, name: String) -> [String: Any] {
    let size = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? NSNumber)?.int64Value ?? 0
    return ["path": url.path, "name": name, "mime": pickedMime(fileExtension: url.pathExtension), "size": size]
}

func shareableText(_ text: String) -> String? {
    let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? nil : trimmed
}

struct PickArgs: Decodable {
    let accept: String?
}

struct SaveFileArgs: Decodable {
    let path: String
    let name: String
    let mime: String
}

struct OpenFileArgs: Decodable {
    let path: String
    let mime: String
}

class ShareArgs: Decodable {
    let text: String
}

// UIKit holds delegates weakly
private func keep(_ delegate: AnyObject, with controller: UIViewController) {
    objc_setAssociatedObject(controller, "ft.files.delegate", delegate, .OBJC_ASSOCIATION_RETAIN)
}

// JPEG without the camera's metadata, so no location
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
            // fails rather than overwrite, the user just takes it again
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
                // the file is gone once this returns, so copy it now
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

// fails if the user backs out, so nothing is reported saved that isn't
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

final class PreviewItem: NSObject, QLPreviewControllerDataSource, QLPreviewItem {
    let previewItemURL: URL?
    let previewItemTitle: String?

    init(url: URL) {
        previewItemURL = url
        previewItemTitle = url.lastPathComponent
    }

    func numberOfPreviewItems(in controller: QLPreviewController) -> Int { 1 }

    func previewController(_ controller: QLPreviewController, previewItemAt index: Int) -> QLPreviewItem { self }
}

func previewable(path: String) -> PreviewItem? {
    guard !path.isEmpty, FileManager.default.fileExists(atPath: path) else { return nil }
    let item = PreviewItem(url: URL(fileURLWithPath: path))
    return QLPreviewController.canPreview(item) ? item : nil
}

extension PlatformPlugin {
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

    // no Downloads folder on iPhone, so let the user pick where
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
            // on iPad the sheet is a popover and needs an anchor
            UIUtils.centerPopover(rootViewController: screen, popoverController: sheet)
            screen.present(sheet, animated: true)
            invoke.resolve()
        }
    }

    @objc public func shareText(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(ShareArgs.self)
        guard let text = shareableText(args.text) else {
            invoke.reject("nothing to share")
            return
        }
        DispatchQueue.main.async { [manager] in
            guard let screen = manager.viewController else {
                invoke.reject("no screen to share from")
                return
            }
            let sheet = UIActivityViewController(activityItems: [text], applicationActivities: nil)
            // on iPad the sheet is a popover and needs an anchor
            UIUtils.centerPopover(rootViewController: screen, popoverController: sheet)
            screen.present(sheet, animated: true)
            invoke.resolve()
        }
    }

    @objc public func openFile(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(OpenFileArgs.self)
        guard let item = previewable(path: args.path) else {
            invoke.reject("that file cannot be shown here")
            return
        }
        DispatchQueue.main.async { [manager] in
            guard let screen = manager.viewController else {
                invoke.reject("no screen to show it on")
                return
            }
            let preview = QLPreviewController()
            preview.dataSource = item
            // QLPreviewController holds its data source weakly
            objc_setAssociatedObject(preview, "ft.preview.item", item, .OBJC_ASSOCIATION_RETAIN)
            screen.present(preview, animated: true)
            invoke.resolve()
        }
    }
}
