import 'package:flutter/material.dart';
import 'data/notifications.dart';
import 'state/app.dart';
import 'ui/connect.dart';
import 'ui/home.dart';
import 'ui/theme.dart';

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
        debugShowCheckedModeBanner: false,
        theme: themeByName(app.themeName).toThemeData(),
        home: app.connected ? HomeShell(app) : ConnectScreen(app),
      ),
    );
  }
}
