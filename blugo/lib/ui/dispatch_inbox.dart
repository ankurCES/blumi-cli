import 'package:flutter/material.dart';
import '../data/saved_server.dart';
import '../state/app.dart';
import 'dispatch_broadcast.dart';
import 'dispatch_thread.dart';
import 'grid_node.dart' show FlowerGlyph;
import 'kit/kit.dart';

/// Telegram-style dispatch inbox: a Broadcast channel + one row per saved node.
/// Tap a row to open that thread.
class DispatchInboxScreen extends StatelessWidget {
  final AppController app;
  const DispatchInboxScreen(this.app, {super.key});

  void _openThread(BuildContext context, SavedServer s) => Navigator.of(context)
      .push(MaterialPageRoute(builder: (_) => DispatchThreadScreen(app, s)));

  void _openBroadcast(BuildContext context) => Navigator.of(context)
      .push(MaterialPageRoute(builder: (_) => BroadcastScreen(app)));

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final t = BlumiTokens.of(context);
    return Scaffold(
      appBar: AppBar(
        title: const GradientText('Dispatch',
            style: TextStyle(fontSize: 19, fontWeight: FontWeight.w800)),
      ),
      body: ListenableBuilder(
        listenable: app,
        builder: (context, _) {
          return ListView(
            padding: const EdgeInsets.symmetric(vertical: 6, horizontal: 6),
            children: [
              // Broadcast channel.
              ListTile(
                shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(t.radiusSm)),
                leading: Container(
                  width: 40,
                  height: 40,
                  decoration: BoxDecoration(
                    gradient: t.brandGradient,
                    borderRadius: BorderRadius.circular(t.radiusSm),
                  ),
                  child: const Icon(Icons.campaign, color: Colors.white, size: 22),
                ),
                title: Row(children: [
                  const Text('Broadcast',
                      style: TextStyle(fontWeight: FontWeight.w700)),
                  const SizedBox(width: 8),
                  const BlumiBadge('ALL'),
                ]),
                subtitle: Text(
                  'message all ${app.servers.length} node${app.servers.length == 1 ? '' : 's'} at once',
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
                trailing: Icon(Icons.chevron_right, color: t.textMuted),
                onTap: () => _openBroadcast(context),
              ),
              Divider(color: cs.onSurface.withValues(alpha: 0.06), height: 8),
              if (app.servers.isEmpty)
                const Padding(
                  padding: EdgeInsets.only(top: 40),
                  child: EmptyState(
                    icon: Icons.dns_outlined,
                    message: 'No gateways yet',
                    hint: 'Add one from the welcome screen to dispatch to it.',
                  ),
                ),
              for (final s in app.servers)
                ListTile(
                  shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(t.radiusSm)),
                  leading: SizedBox(
                    width: 40,
                    height: 40,
                    child: Center(child: FlowerGlyph(size: 30)),
                  ),
                  title: Text(s.name,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(fontWeight: FontWeight.w600)),
                  subtitle: Text(s.endpoint,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(fontSize: 12, color: t.textMuted)),
                  trailing: Icon(Icons.chevron_right, color: t.textMuted),
                  onTap: () => _openThread(context, s),
                ),
            ],
          );
        },
      ),
    );
  }
}
