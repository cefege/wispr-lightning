import AppKit
import SwiftUI

// MARK: - SwiftUI Sign In View

struct SignInView: View {
    @ObservedObject var vm: SignInViewModel

    var body: some View {
        VStack(spacing: Theme.Spacing.xlarge) {
            Spacer()

            Image(systemName: "mic.circle.fill")
                .font(.system(size: 64, weight: .light))
                .foregroundStyle(.tint)

            Text("Welcome to Wispr Lite")
                .font(.title2.weight(.semibold))

            Text("Sign in with your Wispr account to start dictating anywhere on Mac.")
                .font(.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(width: 300)

            ZStack {
                Button("Sign In with Google") {
                    vm.signIn()
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .opacity(vm.isLoading ? 0 : 1)

                if vm.isLoading {
                    ProgressView()
                        .controlSize(.small)
                }
            }

            if let error = vm.errorMessage {
                Text(error)
                    .font(.subheadline)
                    .foregroundStyle(.red)
            }

            Text("Or sign in to Wispr Flow — we'll use that session automatically.")
                .font(.subheadline)
                .foregroundStyle(.tertiary)

            Spacer()
        }
        .padding(Theme.Spacing.xlarge)
        .frame(width: 420, height: 340)
    }
}

// MARK: - View Model

class SignInViewModel: ObservableObject {
    @Published var isLoading = false
    @Published var errorMessage: String?

    func signIn() {
        isLoading = true
        errorMessage = nil
        AuthService.signInWithBrowser()

        DispatchQueue.main.asyncAfter(deadline: .now() + 3.0) { [weak self] in
            self?.isLoading = false
        }
    }

    func showError(_ message: String) {
        errorMessage = message
        isLoading = false
    }
}

// MARK: - Window Controller

class SignInWindow {
    private var window: NSWindow?
    private let viewModel = SignInViewModel()

    func showWindow() {
        if let window = window {
            window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            return
        }

        let signInView = SignInView(vm: viewModel)
        let hostingView = NSHostingView(rootView: signInView)

        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 420, height: 340),
            styleMask: [.titled, .closable],
            backing: .buffered,
            defer: false
        )
        w.title = "Sign In to Wispr Lite"
        w.titleVisibility = .hidden
        w.titlebarAppearsTransparent = true
        w.isMovableByWindowBackground = true
        w.center()
        w.isReleasedWhenClosed = false
        w.contentView = hostingView

        self.window = w
        w.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func showError(_ message: String) {
        viewModel.showError(message)
    }

    func dismiss() {
        window?.close()
    }
}
