import UIKit

/// Minimal host shell: the Rust/eframe editor owns the window and event loop
/// from here on. This runs on the main thread (winit iOS requirement) and
/// never returns while the app is alive.
@main
class AppDelegate: UIResponder, UIApplicationDelegate {
    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        vadadee_berry_ios_main()
        return true
    }
}
