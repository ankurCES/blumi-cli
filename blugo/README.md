# blugo

The **blumi** phone app — a Flutter client that mirrors the [blumi](../README.md) terminal UI on
your phone, talking to a `blumi serve` gateway over your LAN (REST + SSE, token auth). Designed for
the **Pixel 9 Pro Fold**: single-pane in portrait, multi-pane when unfolded (it re-lays out on the
fold transition — it never locks orientation).

| Chat · markdown & code | Approvals · thinking | Control center |
|---|---|---|
| <img src="../docs/screenshots/app-chat.png" alt="chat" width="240"> | <img src="../docs/screenshots/app-approval.png" alt="approval" width="240"> | <img src="../docs/screenshots/app-control.png" alt="control center" width="240"> |

## Features

- **Streaming chat** with markdown + syntax-highlighted code blocks, tool cards, and the animated
  thinking mascot (ported from the TUI).
- **Interactive cards**: approval (allow once / session / deny), clarify, and plan review.
- **Sessions**: list, new, resume; transcript auto-refreshes after each turn (pull-to-refresh too).
- **Control center**: model / persona / theme / YOLO, plus tabs for Status, Tasks, Usage, Skills,
  Memory, and Voice.
- **Voice**: TTS via ElevenLabs or OpenAI (pick a voice from a dropdown after authenticating) and
  mic → text via OpenAI-compatible Whisper.
- **Multi-instance + discovery**: save several gateways by name and auto-discover them on the LAN
  over mDNS (`_blumi._tcp`).
- **Caching**: stale-while-revalidate so views paint instantly from cache, then refresh.

## Prerequisites

- Flutter **3.44.1** (stable) — `flutter --version`. Install: https://docs.flutter.dev/get-started/install
- Android: a device/emulator with USB debugging, JDK 17, and the Android SDK
  (`flutter doctor` should be all green).
- A running **blumi gateway** on your LAN — see the [Gateway guide](https://github.com/ankurCES/blumi-cli/wiki/Gateway):
  ```sh
  blumi serve pair                    # set a password + print the LAN URL/QR
  blumi serve install --host <LAN-ip> # always-on service
  ```

## Run (debug)

```sh
cd blugo
flutter pub get
flutter run -d <device>     # `flutter devices` to list
```

On first launch, blugo lists gateways discovered on your Wi-Fi (or add one by `IP:port`); enter
the gateway password to connect. The same session is live in the TUI, the web UI, and the phone.

## Build a release APK

Release builds are signed from a keystore referenced by `android/key.properties` (both are
**gitignored** — never commit them). To create your own:

```sh
keytool -genkey -v -keystore android/app/blugo-release.jks \
  -keyalg RSA -keysize 2048 -validity 10000 -alias blugo
```

`android/key.properties`:
```properties
storeFile=blugo-release.jks
storePassword=<your-store-password>
keyPassword=<your-key-password>
keyAlias=blugo
```

Then:
```sh
flutter build apk --release
# → build/app/outputs/flutter-apk/app-release.apk
adb install -r build/app/outputs/flutter-apk/app-release.apk
```

If `key.properties` is absent the build falls back to debug signing, so the repo still builds for
contributors without the keystore. **Keep the keystore safe** — you need the same one to ship app
updates.

## Architecture

MVVM with `ChangeNotifier` + `ListenableBuilder`/`AnimatedBuilder`:

```
lib/
  data/      api.dart (REST), sse.dart (event stream), models.dart, session.dart,
             cache.dart (SWR), voice.dart, elevenlabs.dart, saved_server.dart, events.dart
  state/     app.dart (AppController: servers, meta, theme), session.dart (BlumiSession)
  ui/        connect.dart, home.dart (chat/composer/cards), control.dart (control center),
             markdown.dart, thinking.dart (mascot), theme.dart
```

## Tests

```sh
flutter analyze
flutter test
```

More: the **[Mobile App](https://github.com/ankurCES/blumi-cli/wiki/Mobile-App)** wiki page.
