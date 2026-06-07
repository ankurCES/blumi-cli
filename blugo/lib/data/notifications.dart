import 'dart:async';
import 'package:flutter/widgets.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';

/// Local "run finished" notifications (#209c) — the phone analog of the web
/// in-tab alert. When a turn you started completes while blugo is **not in the
/// foreground**, fire a heads-up local notification so you can come back to it.
///
/// Best-effort: a missing permission or an unsupported platform just makes it a
/// no-op. Nothing fires while the app is on screen (it would be redundant), and
/// a `done` only fires if a turn was [arm]ed — so a stale `done` replayed from
/// the SSE backlog on reconnect can't notify spuriously.
class NotificationService with WidgetsBindingObserver {
  NotificationService._();
  static final NotificationService instance = NotificationService._();

  final FlutterLocalNotificationsPlugin _plugin =
      FlutterLocalNotificationsPlugin();
  bool _ready = false;
  bool _foreground = true;
  bool _armed = false;

  static const String _channelId = 'blumi_completion';

  /// Initialize the plugin, register the lifecycle observer, and request the
  /// runtime notification permission (Android 13+ / iOS). Safe to call once at
  /// startup; failures leave the service a no-op.
  Future<void> init() async {
    WidgetsBinding.instance.addObserver(this);
    const android = AndroidInitializationSettings('@mipmap/ic_launcher');
    const ios = DarwinInitializationSettings();
    const settings = InitializationSettings(android: android, iOS: ios);
    try {
      await _plugin.initialize(settings);
      await _plugin
          .resolvePlatformSpecificImplementation<
              AndroidFlutterLocalNotificationsPlugin>()
          ?.requestNotificationsPermission();
      await _plugin
          .resolvePlatformSpecificImplementation<
              IOSFlutterLocalNotificationsPlugin>()
          ?.requestPermissions(alert: true, badge: true, sound: true);
      _ready = true;
    } catch (_) {
      _ready = false;
    }
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    _foreground = state == AppLifecycleState.resumed;
  }

  /// Arm on turn start. A completion only notifies if a turn was armed.
  void arm() => _armed = true;

  /// Notify iff a turn was armed AND the app is backgrounded. Disarms either way.
  Future<void> notifyCompletionIfBackground(String title, String body) async {
    final wasArmed = _armed;
    _armed = false;
    if (!wasArmed || _foreground || !_ready) return;
    const android = AndroidNotificationDetails(
      _channelId,
      'Run completion',
      channelDescription: 'Notifies when a blumi run finishes.',
      importance: Importance.high,
      priority: Priority.high,
    );
    const details =
        NotificationDetails(android: android, iOS: DarwinNotificationDetails());
    try {
      await _plugin.show(0, title, body, details);
    } catch (_) {
      // Best-effort.
    }
  }
}
