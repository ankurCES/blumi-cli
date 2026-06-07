import 'package:flutter/material.dart';
import '../state/app.dart';
import 'grid_map.dart';
import 'kit/kit.dart';
import 'node_sheets.dart';

/// First screen: an interactive grid diagram of this device plus every saved and
/// auto-discovered blumi gateway. Tap a node to connect/edit/forget it, tap a
/// discovered (dashed) node to sign in, or use ＋ to add one by address.
class ConnectScreen extends StatefulWidget {
  final AppController app;
  const ConnectScreen(this.app, {super.key});

  @override
  State<ConnectScreen> createState() => _ConnectScreenState();
}

class _ConnectScreenState extends State<ConnectScreen> {
  bool _reauthOpen = false;

  @override
  void initState() {
    super.initState();
    widget.app.startDiscovery();
    widget.app.addListener(_maybeReauth);
    WidgetsBinding.instance.addPostFrameCallback((_) => _maybeReauth());
  }

  @override
  void dispose() {
    widget.app.removeListener(_maybeReauth);
    widget.app.stopDiscovery();
    super.dispose();
  }

  /// When a saved token goes stale (e.g. on auto-connect), prompt for the
  /// password for that gateway. Guarded so we open at most one prompt.
  void _maybeReauth() {
    final app = widget.app;
    if (!mounted || _reauthOpen) return;
    final srv = app.reauthFor;
    if (srv == null || app.connected) return;
    _reauthOpen = true;
    showReauthSheet(context, app, srv).whenComplete(() {
      _reauthOpen = false;
      // If the user dismissed without connecting, clear the pending re-auth so
      // the diagram returns to its normal state.
      if (widget.app.reauthFor != null && !widget.app.connected) {
        widget.app.cancelReauth();
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final app = widget.app;
    final cs = Theme.of(context).colorScheme;
    final t = BlumiTokens.of(context);

    return Scaffold(
      floatingActionButton: ListenableBuilder(
        listenable: app,
        builder: (context, _) => FloatingActionButton.extended(
          onPressed: app.connecting ? null : () => showAddNodeSheet(context, app),
          backgroundColor: cs.primary,
          foregroundColor: Colors.black,
          icon: const Icon(Icons.add),
          label: const Text('Add'),
        ),
      ),
      body: SafeArea(
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 16, 20, 2),
              child: Row(
                children: [
                  Hero(
                    tag: heroLogoTag,
                    child: Image.asset(
                      'assets/icon/blugo_mark.png',
                      width: 46,
                      height: 46,
                      filterQuality: FilterQuality.medium,
                    ),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        GradientText(
                          'blugo',
                          style: const TextStyle(
                            fontSize: 30,
                            fontWeight: FontWeight.w800,
                            height: 1.0,
                          ),
                        ),
                        const SizedBox(height: 2),
                        ListenableBuilder(
                          listenable: app,
                          builder: (context, _) => Text(
                            app.connecting && app.status.isNotEmpty
                                ? app.status
                                : 'your blumi grid',
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style:
                                TextStyle(color: t.textMuted, fontSize: 12.5),
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
            Expanded(child: GridMap(app)),
          ],
        ),
      ),
    );
  }
}
