import 'package:flutter/material.dart';
import '../data/saved_server.dart';
import '../state/app.dart';

/// First screen: pick a saved blumi gateway, add a new one, or re-auth one
/// whose token went stale. blugo can hold several machines, each named.
class ConnectScreen extends StatefulWidget {
  final AppController app;
  const ConnectScreen(this.app, {super.key});

  @override
  State<ConnectScreen> createState() => _ConnectScreenState();
}

class _ConnectScreenState extends State<ConnectScreen> {
  final _name = TextEditingController();
  final _host = TextEditingController();
  final _port = TextEditingController(text: '7777');
  final _pass = TextEditingController();
  bool _adding = false;

  @override
  void dispose() {
    _name.dispose();
    _host.dispose();
    _port.dispose();
    _pass.dispose();
    super.dispose();
  }

  void _add() {
    widget.app.addAndConnect(
      name: _name.text,
      host: _host.text.trim(),
      port: int.tryParse(_port.text.trim()) ?? 7777,
      password: _pass.text,
    );
  }

  @override
  Widget build(BuildContext context) {
    final app = widget.app;
    final cs = Theme.of(context).colorScheme;
    return Scaffold(
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 460),
          child: ListenableBuilder(
            listenable: app,
            builder: (context, _) {
              final reauth = app.reauthFor;
              final showForm = app.servers.isEmpty || _adding;
              return ListView(
                shrinkWrap: true,
                padding: const EdgeInsets.all(24),
                children: [
                  Center(
                    child: Image.asset('assets/icon/blugo_mark.png',
                        width: 88,
                        height: 88,
                        filterQuality: FilterQuality.medium),
                  ),
                  const SizedBox(height: 10),
                  Center(
                    child: Text('blugo',
                        style: TextStyle(
                            fontSize: 42,
                            fontWeight: FontWeight.bold,
                            color: cs.primary)),
                  ),
                  const SizedBox(height: 4),
                  Center(
                    child: Text(
                      reauth != null
                          ? 'sign in to ${reauth.name}'
                          : 'connect to your blumi gateways',
                      style:
                          TextStyle(color: cs.onSurface.withValues(alpha: 0.6)),
                    ),
                  ),
                  const SizedBox(height: 24),
                  if (reauth != null)
                    ..._reauthBody(app, reauth, cs)
                  else ...[
                    if (app.servers.isNotEmpty) ..._serverList(app, cs),
                    if (showForm)
                      ..._addForm(app, cs)
                    else
                      Padding(
                        padding: const EdgeInsets.only(top: 8),
                        child: OutlinedButton.icon(
                          onPressed: () => setState(() => _adding = true),
                          icon: const Icon(Icons.add),
                          label: const Text('Add another gateway'),
                        ),
                      ),
                  ],
                  if (app.status.isNotEmpty && !app.connecting && reauth == null)
                    Padding(
                      padding: const EdgeInsets.only(top: 14),
                      child: Text(app.status,
                          style: TextStyle(color: cs.error),
                          textAlign: TextAlign.center),
                    ),
                  const SizedBox(height: 22),
                  Text(
                    'On each Mac:  blumi serve pair --password <pw>\n'
                    'then  blumi serve install',
                    style: TextStyle(
                        fontFamily: 'monospace',
                        fontSize: 12,
                        color: cs.onSurface.withValues(alpha: 0.45)),
                    textAlign: TextAlign.center,
                  ),
                ],
              );
            },
          ),
        ),
      ),
    );
  }

  // --- saved gateways list ---------------------------------------------------

  List<Widget> _serverList(AppController app, ColorScheme cs) => [
        Align(
          alignment: Alignment.centerLeft,
          child: Text('saved gateways',
              style: TextStyle(
                  fontWeight: FontWeight.bold,
                  color: cs.onSurface.withValues(alpha: 0.7),
                  fontSize: 13)),
        ),
        const SizedBox(height: 6),
        for (final s in app.servers)
          Card(
            margin: const EdgeInsets.symmetric(vertical: 4),
            child: ListTile(
              leading: Icon(Icons.dns_outlined, color: cs.primary),
              title: Text(s.name,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(fontWeight: FontWeight.w600)),
              subtitle: Text(s.endpoint, style: const TextStyle(fontSize: 12)),
              trailing: IconButton(
                tooltip: 'Forget',
                icon: const Icon(Icons.delete_outline),
                onPressed: app.connecting ? null : () => _confirmForget(app, s),
              ),
              onTap: app.connecting ? null : () => app.connectToSaved(s),
            ),
          ),
        const SizedBox(height: 8),
      ];

  Future<void> _confirmForget(AppController app, SavedServer s) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Forget ${s.name}?'),
        content: Text('Removes ${s.endpoint} and its saved token.'),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('Cancel')),
          FilledButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: const Text('Forget')),
        ],
      ),
    );
    if (ok == true) app.removeServer(s.id);
  }

  // --- add a gateway ---------------------------------------------------------

  List<Widget> _addForm(AppController app, ColorScheme cs) => [
        if (app.servers.isNotEmpty)
          Align(
            alignment: Alignment.centerLeft,
            child: Text('add a gateway',
                style: TextStyle(
                    fontWeight: FontWeight.bold,
                    color: cs.onSurface.withValues(alpha: 0.7),
                    fontSize: 13)),
          ),
        const SizedBox(height: 6),
        TextField(
          controller: _name,
          decoration: const InputDecoration(
            labelText: 'Name (optional)',
            hintText: 'defaults to the machine name',
            border: OutlineInputBorder(),
          ),
        ),
        const SizedBox(height: 12),
        TextField(
          controller: _host,
          autocorrect: false,
          decoration: const InputDecoration(
            labelText: 'Host (Mac LAN IP)',
            hintText: '10.0.0.61',
            border: OutlineInputBorder(),
          ),
        ),
        const SizedBox(height: 12),
        TextField(
          controller: _port,
          keyboardType: TextInputType.number,
          decoration: const InputDecoration(
              labelText: 'Port', border: OutlineInputBorder()),
        ),
        const SizedBox(height: 12),
        TextField(
          controller: _pass,
          obscureText: true,
          decoration: const InputDecoration(
              labelText: 'Password', border: OutlineInputBorder()),
          onSubmitted: (_) => _add(),
        ),
        const SizedBox(height: 16),
        Row(
          children: [
            Expanded(
              child: FilledButton(
                onPressed: app.connecting ? null : _add,
                child: Padding(
                  padding: const EdgeInsets.symmetric(vertical: 6),
                  child: Text(app.connecting ? 'Connecting…' : 'Connect'),
                ),
              ),
            ),
            if (app.servers.isNotEmpty) ...[
              const SizedBox(width: 8),
              TextButton(
                onPressed: app.connecting
                    ? null
                    : () => setState(() => _adding = false),
                child: const Text('Cancel'),
              ),
            ],
          ],
        ),
      ];

  // --- re-auth a stale gateway ----------------------------------------------

  List<Widget> _reauthBody(AppController app, SavedServer s, ColorScheme cs) => [
        Card(
          margin: const EdgeInsets.only(bottom: 12),
          child: ListTile(
            leading: Icon(Icons.dns_outlined, color: cs.primary),
            title: Text(s.name,
                style: const TextStyle(fontWeight: FontWeight.w600)),
            subtitle: Text(s.endpoint, style: const TextStyle(fontSize: 12)),
          ),
        ),
        TextField(
          controller: _pass,
          obscureText: true,
          autofocus: true,
          decoration: const InputDecoration(
              labelText: 'Password', border: OutlineInputBorder()),
          onSubmitted: (_) => app.reauthenticate(_pass.text),
        ),
        if (app.status.isNotEmpty && !app.connecting)
          Padding(
            padding: const EdgeInsets.only(top: 12),
            child: Text(app.status,
                style: TextStyle(color: cs.error), textAlign: TextAlign.center),
          ),
        const SizedBox(height: 16),
        Row(
          children: [
            Expanded(
              child: FilledButton(
                onPressed:
                    app.connecting ? null : () => app.reauthenticate(_pass.text),
                child: Padding(
                  padding: const EdgeInsets.symmetric(vertical: 6),
                  child: Text(app.connecting ? 'Connecting…' : 'Connect'),
                ),
              ),
            ),
            const SizedBox(width: 8),
            TextButton(
              onPressed: app.connecting
                  ? null
                  : () {
                      _pass.clear();
                      app.cancelReauth();
                    },
              child: const Text('Back'),
            ),
          ],
        ),
      ];
}
