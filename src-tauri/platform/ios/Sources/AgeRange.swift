import Foundation
import Tauri
import UIKit
#if canImport(DeclaredAgeRange)
import DeclaredAgeRange
#endif

// Minor or adult, as the system says it (Ioan, 2026-10-07; Declared Age Range, iOS 26+). Never a
// date of birth, and the answer never leaves the phone (§30, §43). Minors are always free; an
// adult, or someone the system cannot place, pays after the free year.

/// The single age gate the app asks with: below it a minor, from it an adult.
let adultAge = 18

/// `minor`, `adult` or `unknown` from what the system answered: whether the person shared and the
/// range's bounds (`lower` nil below the lowest gate, `upper` nil from the highest). The system may
/// use its region's own gates; a range that straddles 18 says nothing.
func ageClassOf(sharing: Bool, lower: Int?, upper: Int?) -> String {
    guard sharing else { return "unknown" }
    if let upper = upper, upper < adultAge { return "minor" }
    if let lower = lower, lower >= adultAge { return "adult" }
    return "unknown"
}

extension PlatformPlugin {
    /// Asks the system once (it shows its own sheet) and answers `{age: "minor" | "adult" |
    /// "unknown"}`. Before iOS 26, declined, no account or an error: `unknown`. Nothing is kept.
    @objc public func ageClass(_ invoke: Invoke) throws {
        #if canImport(DeclaredAgeRange)
        if #available(iOS 26.0, *) {
            Task { @MainActor [manager] in
                invoke.resolve(["age": await askSystemForAge(on: manager.viewController)])
            }
            return
        }
        #endif
        invoke.resolve(["age": "unknown"])
    }
}

#if canImport(DeclaredAgeRange)
@available(iOS 26.0, *)
@MainActor
private func askSystemForAge(on screen: UIViewController?) async -> String {
    guard let screen = screen else { return "unknown" }
    do {
        switch try await AgeRangeService.shared.requestAgeRange(ageGates: adultAge, in: screen) {
        case .sharing(let range):
            return ageClassOf(sharing: true, lower: range.lowerBound, upper: range.upperBound)
        case .declinedSharing:
            return "unknown"
        @unknown default:
            return "unknown"
        }
    } catch {
        return "unknown"
    }
}
#endif
