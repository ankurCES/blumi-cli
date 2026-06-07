import 'package:flutter/material.dart';
import '../data/saved_server.dart';
import '../state/app.dart';
import 'home.dart' show ChatPane;
import 'kit/kit.dart';
import 'node_sheets.dart';

/// A lightweight Telegram-style chat with one node, on its dedicated dispatch
/// session. Reuses the full ChatPane (bubbles, tool/approval/plan cards,
/// composer, voice) — it just needs a BlumiSession.
class DispatchThreadScreen extends StatelessWidget {
  final AppController app;
  final SavedServer server;
  const DispatchThreadScreen(this.app, this.server, {super.key});

  @override
  Widget build(BuildContext context) {
    final t = BlumiTokens.of(context);
    return Scaffold(
      appBar: AppBar(
        titleSpacing: 6,
        title: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(server.name,
                style:
                    const TextStyle(fontSize: 16, fontWeight: FontWeight.w700)),
            Text('dispatch · ${server.endpoint}',
                style: TextStyle(fontSize: 11, color: t.textMuted)),
          ],
        ),
      ),
      body: server.token == null
          ? Center(
              child: EmptyState(
                icon: Icons.lock_outline,
                message: 'Sign in to ${server.name} first',
                hint: 'Dispatch needs an authenticated gateway.',
                action: GradientButton(
                  label: 'Sign in',
                  icon: Icons.bolt,
                  expand: false,
                  onPressed: () => showReauthSheet(context, app, server),
                ),
              ),
            )
          : ChatPane(app.dispatch.openFor(server)),
    );
  }
}
