import 'dart:convert';
import 'package:firebase_core/firebase_core.dart';
import 'package:firebase_messaging/firebase_messaging.dart';
import 'notifications.dart';

/// FCM background handler — must be a top-level, `vm:entry-point` function.
/// We send notification+data messages, so the OS shows the tray item while the
/// app is backgrounded/terminated; routing happens on tap. Nothing to do here.
@pragma('vm:entry-point')
Future<void> _firebaseBgHandler(RemoteMessage message) async {}

/// Owns the Firebase Cloud Messaging lifecycle: optional init (a graceful no-op
/// when Firebase isn't configured, so the app still runs on local notifications),
/// the device token handoff, and routing a notification tap to a dispatch thread.
class PushService {
  PushService._();
  static final PushService instance = PushService._();

  bool _enabled = false;
  bool get enabled => _enabled;

  String? _token;
  String? get token => _token;

  /// Called with a fresh token on acquisition/refresh (register it with gateways).
  void Function(String token)? onTokenChanged;

  /// Called when a push is tapped, with the dispatch `session_id` + `node`.
  void Function(String sessionId, String node)? onOpenThread;

  /// Initialize FCM. Safe to call once at startup; if Firebase isn't configured
  /// (no `google-services.json`), this is a no-op and [enabled] stays false.
  Future<void> init(
      {void Function(String sessionId, String node)? onOpenThread}) async {
    this.onOpenThread = onOpenThread;
    try {
      await Firebase.initializeApp();
      final fm = FirebaseMessaging.instance;
      FirebaseMessaging.onBackgroundMessage(_firebaseBgHandler);
      await fm.requestPermission();

      // Foreground: the OS won't surface a notification message while the app is
      // open, so show it via the local plugin (its tap routes through onTap).
      FirebaseMessaging.onMessage.listen((m) {
        final n = m.notification;
        final title = n?.title ?? m.data['title']?.toString() ?? 'blumi';
        final body = n?.body ?? m.data['body']?.toString() ?? '';
        NotificationService.instance
            .show(title, body, payload: jsonEncode(m.data));
      });

      // Tap on a tray notification (app backgrounded or cold-started).
      FirebaseMessaging.onMessageOpenedApp.listen(_routeTap);
      final initial = await fm.getInitialMessage();
      if (initial != null) _routeTap(initial);

      _token = await fm.getToken();
      fm.onTokenRefresh.listen((t) {
        _token = t;
        onTokenChanged?.call(t);
      });
      _enabled = _token != null;
      if (_enabled) onTokenChanged?.call(_token!);
    } catch (_) {
      _enabled = false; // No Firebase config → local notifications only.
    }
  }

  void _routeTap(RemoteMessage m) {
    final sid = m.data['session_id']?.toString();
    final node = m.data['node']?.toString() ?? '';
    if (sid != null && sid.isNotEmpty) onOpenThread?.call(sid, node);
  }
}
