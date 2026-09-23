# Android quick-toggle widget plan

## Goal and first release

Build a small native Android application whose home-screen widget toggles one
configured WiZ bulb directly over the same local LAN as the phone. Version 1 is
intentionally narrow:

- one bulb, identified by a user-entered IPv4 address;
- one widget action: read current power, then turn it on or off;
- visible `on`, `off`, `working`, and `unreachable` states;
- no account, cloud service, analytics, background polling, or Internet access;
- an app settings screen for IP, connection test, and optional explicit
  on/off actions for diagnosing toggle races.

The phone must be on a network that can reach the bulb. Guest-network client
isolation, VPN routing, cellular data, and some mesh/router settings can prevent
local UDP access even when the configuration is correct.

## Recommended implementation

Create an `android/` Gradle project using Kotlin. Use an Android home-screen
`AppWidgetProvider` with Glance for rendering, a coroutine-backed worker for
network work, and a small pure Kotlin WiZ protocol module. Do not embed Python,
shell out to the CLI, or depend on a desktop process.

The widget click path should be:

1. Disable/relabel the clicked widget as `working` immediately.
2. Enqueue a unique, expedited one-shot operation so repeated taps coalesce.
3. Send WiZ JSON-RPC `getPilot` by UDP to the configured address on port 38899.
4. Parse the returned `state` value.
5. Send `setPilot` containing only `{ "state": !currentState }`; never replay
   cached brightness, scene, RGB, or temperature fields.
6. Read back `getPilot`, store the observed result, and refresh every widget
   instance.
7. On timeout, show `unreachable` without guessing or flipping cached state.

Use a short bounded retry policy (for example, two attempts within a few
seconds). A toggle is a read-then-write operation and WiZ LAN control offers no
atomic compare-and-set, so concurrent control from the WiZ app, a wall switch,
or automation can still race. The UI must treat the final readback as truth.

## Modules and files

```text
android/
├── app/src/main/AndroidManifest.xml
├── app/src/main/java/.../
│   ├── MainActivity.kt             # IP setup and Test Connection
│   ├── data/BulbPreferences.kt     # validated configuration/state cache
│   ├── protocol/WizClient.kt       # UDP transport, timeout, request IDs
│   ├── protocol/WizMessages.kt     # JSON request/response models
│   ├── toggle/ToggleBulbUseCase.kt # read, delta-write, readback
│   ├── widget/WizWidget.kt         # Glance UI
│   ├── widget/ToggleAction.kt      # click handling and work scheduling
│   └── widget/WidgetUpdater.kt     # state/error rendering
└── app/src/test/                   # protocol and use-case tests
```

Keep protocol messages independent from Android framework classes. That makes
them easy to unit-test and lets both the Python/Rust code and Android tests use
the same checked-in JSON fixtures.

## Android-specific requirements

- Declare network access and Wi-Fi/network-state permissions required by the
  chosen Android SDK level. Do not request Internet-facing services or broad
  device permissions.
- Perform UDP work off the main thread and bind every socket receive to a
  timeout. Always close sockets.
- Prefer unicast to the configured IP. Discovery by broadcast/multicast can be
  a later feature because it introduces additional Wi-Fi/multicast handling,
  permissions, and OEM-specific behavior.
- Store only non-secret local settings. Validate the address before saving it;
  do not accept a hostname or arbitrary port in version 1.
- Give the widget an accessible content description and ensure state is not
  conveyed by color alone.
- Use unique work keyed by bulb/widget so rapid taps cannot create a queue of
  stale toggles.

## Delivery milestones

### Milestone 0 — settle the core direction

Choose whether `main` remains the Python CLI/library or is replaced by the Rust
desktop application. This does not block a native Android client, but it decides
which implementation supplies the canonical protocol fixtures and semantics.

### Milestone 1 — protocol spike

- Capture sanitized `getPilot` and `setPilot` request/response fixtures.
- Implement the Kotlin UDP client and power-only delta command.
- Test malformed JSON, mismatched request IDs, packet loss, timeout, and late
  responses with a local fake UDP bulb.
- Prove the spike against one real bulb from an Android device on Wi-Fi.

**Exit criterion:** a command-line/instrumented test performs 20 alternating
on/off operations with correct readback and no leaked sockets.

### Milestone 2 — setup application

- Add IP entry, validation, persistence, and `Test Connection`.
- Show the bulb's actual power state and actionable timeout/network messages.
- Add a link to Android's widget picker or simple placement instructions.

**Exit criterion:** a fresh install can be configured without developer tools.

### Milestone 3 — widget MVP

- Implement the Glance widget and `working`/`on`/`off`/`unreachable` rendering.
- Coalesce rapid taps and apply read-before-write plus final readback.
- Update all placed widget instances after settings or state changes.

**Exit criterion:** toggle works after process death and phone restart, and a
failed request never displays a fabricated power state.

### Milestone 4 — quality and release

- Unit-test protocol serialization and toggle race/error behavior.
- Add Android emulator tests for configuration and widget actions.
- Test on at least two Android API levels and one physical phone/bulb/router.
- Add Gradle build/lint/test jobs to CI and produce a signed internal APK.
- Document LAN-only behavior, privacy, troubleshooting, and supported Android
  versions.

**Exit criterion:** CI is green, the release build installs cleanly, airplane
mode/Wi-Fi loss is handled, and the widget recovers after connectivity returns.

## Deferred features

After the toggle MVP is reliable, add multiple bulbs/widgets, separate on/off
buttons, brightness presets, discovery, scenes, and Quick Settings tiles in
that order. A Quick Settings tile may ultimately be even faster than a
home-screen widget, but it should reuse the same tested toggle use case rather
than introduce a second network implementation.

