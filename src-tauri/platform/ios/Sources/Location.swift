import CoreLocation
import Foundation

// The location plugin (2026-10-02, Plan §53): the phone's current position, once, while the app
// is open, for a plugin the user granted `location` (the core checked before asking here). Only
// "when in use": no "always", no background mode, no updates after the one fix, and nothing of it
// is kept or logged.

enum LocationAccess: Equatable {
    case granted
    case ask
    case denied
}

/// How long the user waits for a fix, once the phone may look, before the plugin gets nothing.
let locationTimeout: TimeInterval = 15

/// Asked only if it was never asked; a no (or a restriction, such as Screen Time) stays a no.
/// "Always" was never asked for, but if the user gave it in Settings it is still a yes.
func locationAccess(_ status: CLAuthorizationStatus) -> LocationAccess {
    switch status {
    case .authorizedWhenInUse, .authorizedAlways: return .granted
    case .notDetermined: return .ask
    case .denied, .restricted: return .denied
    @unknown default: return .denied
    }
}

/// What `currentLocation` resolves with (`Located` in platform/src/lib.rs). A negative accuracy
/// is CoreLocation saying the fix is not valid: nothing.
func locationPayload(_ location: CLLocation?) -> [String: Any] {
    guard let location, location.horizontalAccuracy >= 0 else { return ["found": false] }
    return [
        "found": true,
        "lat": location.coordinate.latitude,
        "lon": location.coordinate.longitude,
        "accuracy": location.horizontalAccuracy,
        "at": (location.timestamp.timeIntervalSince1970 * 1000).rounded(),
    ]
}

/// One request for the current position: asks "when in use" if it was never asked, then one fix
/// (`requestLocation`), or nothing after `locationTimeout`. Runs on the main thread, where
/// CLLocationManager wants to live; the request keeps itself alive until it answers, once.
final class LocationRequest: NSObject, CLLocationManagerDelegate {
    private static var running: [LocationRequest] = []

    private let manager = CLLocationManager()
    private let done: ([String: Any]) -> Void
    private var answered = false
    private var looking = false

    private init(done: @escaping ([String: Any]) -> Void) {
        self.done = done
        super.init()
    }

    /// Call on the main thread.
    static func start(done: @escaping ([String: Any]) -> Void) {
        let request = LocationRequest(done: done)
        running.append(request)
        request.begin()
    }

    private func begin() {
        manager.delegate = self
        manager.desiredAccuracy = kCLLocationAccuracyNearestTenMeters
        decide(manager.authorizationStatus)
    }

    private func decide(_ status: CLAuthorizationStatus) {
        switch locationAccess(status) {
        case .granted: look()
        case .ask: manager.requestWhenInUseAuthorization() // answers in the delegate below
        case .denied: finish(nil)
        }
    }

    /// The phone may look now: one fix, and the clock starts (the user's time on the question
    /// does not count).
    private func look() {
        guard !looking else { return }
        looking = true
        manager.requestLocation()
        DispatchQueue.main.asyncAfter(deadline: .now() + locationTimeout) { [weak self] in self?.finish(nil) }
    }

    private func finish(_ location: CLLocation?) {
        guard !answered else { return }
        answered = true
        manager.stopUpdatingLocation()
        manager.delegate = nil
        done(locationPayload(location))
        LocationRequest.running.removeAll { $0 === self }
    }

    func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        guard !answered, !looking else { return }
        // Still `notDetermined` while the question is on the screen: wait for the answer.
        if manager.authorizationStatus != .notDetermined { decide(manager.authorizationStatus) }
    }

    func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        finish(locations.last)
    }

    func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {
        finish(nil)
    }
}
