import CoreLocation
import Foundation

// One fix, "when in use" only. Nothing is kept or logged.

enum LocationAccess: Equatable {
    case granted
    case ask
    case denied
}

let locationTimeout: TimeInterval = 15

// "always" was never asked for, but if the user set it in Settings it's still a yes
func locationAccess(_ status: CLAuthorizationStatus) -> LocationAccess {
    switch status {
    case .authorizedWhenInUse, .authorizedAlways: return .granted
    case .notDetermined: return .ask
    case .denied, .restricted: return .denied
    @unknown default: return .denied
    }
}

// negative accuracy means CoreLocation has no valid fix
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

// CLLocationManager wants the main thread. The request keeps itself alive until it answers.
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
        case .ask: manager.requestWhenInUseAuthorization()
        case .denied: finish(nil)
        }
    }

    // start the clock here, the user's time on the permission prompt doesn't count
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
        // still notDetermined while the prompt is up, wait for the delegate
        if manager.authorizationStatus != .notDetermined { decide(manager.authorizationStatus) }
    }

    func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        finish(locations.last)
    }

    func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {
        finish(nil)
    }
}
