# Mailrs — native SwiftUI client

Cross-platform (iOS 17+, iPadOS 17+, macOS 14+) native client for the mailrs
backend. One codebase, three platforms. See
`../.claude/rfcs/20260421-macapp-swiftui-port.md` for the v1 RFC / roadmap.

**Status:** M1 (login shell). M2–M5 pending.

## Prerequisites

- Xcode 16+ (tested with Xcode 26.4)
- [xcodegen](https://github.com/yonaskolb/XcodeGen) — `brew install xcodegen`

The `Mailrs.xcodeproj` is generated from `project.yml` — do not edit it by
hand. Regenerate with `xcodegen generate` after touching `project.yml` or
`Config/*.xcconfig`.

## Build & test (CLI)

```bash
cd macapp
xcodegen generate

# macOS (native)
xcodebuild -scheme Mailrs -destination 'platform=macOS' test

# iOS Simulator
xcodebuild -scheme Mailrs \
  -destination 'platform=iOS Simulator,name=iPhone 17 Pro' test

# iPadOS (generic iOS Simulator destination covers it)
xcodebuild -scheme Mailrs \
  -destination 'generic/platform=iOS Simulator' build
```

Local builds use ad-hoc signing (no developer team required). Set
`DEVELOPMENT_TEAM` in a local override xcconfig for TestFlight / distribution.

## Layout

```
Mailrs/
├── MailrsApp.swift      @main
├── App/                 AppModel, RootView
├── Core/
│   ├── Networking/      ApiClient (actor), Endpoint, JSONCoders, ApiError
│   ├── Auth/            AuthStore, KeychainTokenStore, AuthService
│   ├── Realtime/        M5 — WebSocket client
│   ├── Persistence/     Settings store
│   └── Util/            Backoff, Throttle, etc.
├── Models/              Codable DTOs (snake_case matches server)
├── Services/            Domain services over ApiClient (M2+)
├── Features/
│   ├── Login/           LoginView + LoginModel (TOTP two-step)
│   ├── MailShell/       Placeholder in M1; NavigationSplitView in M2
│   ├── ConversationList/  M2
│   ├── Thread/          M3
│   ├── Compose/         M4
│   └── Settings/        M3
└── Platform/            #if os(...) shims
```

## Config

- `Config/Shared.xcconfig` — compiler flags, hardening
- `Config/Debug.xcconfig` — local dev (ad-hoc signing, no team required)
- `Config/Release.xcconfig` — release build
- API base URL is in `Core/Networking/AppConfig.swift` (compile-time `#if DEBUG`)

## Testing notes

- Unit tests use Swift Testing (`@Test` macros, Xcode 16+).
- `MailrsTests/Networking/` — `URLProtocol` stub for `ApiClient`.
- `MailrsTests/Models/` — Codable round-trips including Unix-seconds dates.
- `MailrsTests/Auth/` — Keychain tests auto-skip when the test host lacks a
  `keychain-access-groups` entitlement (i.e. unsigned local builds). Sign the
  bundle or run on a simulator with proper entitlements to exercise them.
