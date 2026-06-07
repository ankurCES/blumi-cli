import 'dart:convert';
import 'package:flutter/material.dart';
import 'data/notifications.dart';
import 'data/push.dart';
import 'state/app.dart';
import 'ui/connect.dart';
import 'ui/home.dart';
import 'ui/kit/kit.dart';
import 'ui/theme.dart';

/// Global navigator so a push tap (which arrives without a BuildContext) can
/// route to a dispatch thread.
final GlobalKey<NavigatorState> navigatorKey = GlobalKey<NavigatorState>();

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  // Local completion notifications (#209c) — best-effort; a denied permission
  // just makes it a no-op.
  await NotificationService.instance.init();
  runApp(const BlugoApp());
}

class BlugoApp extends StatefulWidget {
  const BlugoApp({super.key});
  @override
  State<BlugoApp> createState() => _BlugoAppState();
}

class _BlugoAppState extends State<BlugoApp> {
  final app = AppController();

  @override
  void initState() {
    super.initState();
    app.tryAutoConnect();
    _initPush();
  }

  /// Wire FCM: register the device token with gateways, and route a tapped push
  /// (or a tapped foreground-shown local notification) to its dispatch thread.
  void _initPush() {
    PushService.instance.onTokenChanged = app.registerFcmEverywhere;
    NotificationService.instance.onTap = (payload) {
      try {
        final data = jsonDecode(payload) as Map<String, dynamic>;
        final sid = data['session_id']?.toString();
        final node = data['node']?.toString() ?? '';
        if (sid != null && sid.isNotEmpty) app.openDispatchFromPush(sid, node);
      } catch (_) {}
    };
    PushService.instance.init(
      onOpenThread: (sid, node) => app.openDispatchFromPush(sid, node),
    );
  }

  @override
  void dispose() {
    app.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AnimatedBuilder(
      animation: app,
      builder: (context, _) => MaterialApp(
        title: 'blugo',
        navigatorKey: navigatorKey,
        debugShowCheckedModeBanner: false,
        theme: themeByName(app.themeName).toThemeData(),
        home: FadeSwitcher(
          child: app.connected
              ? KeyedSubtree(key: const ValueKey('home'), child: HomeShell(app))
              : KeyedSubtree(
                  key: const ValueKey('connect'), child: ConnectScreen(app)),
        ),
      ),
    );
  }
}
