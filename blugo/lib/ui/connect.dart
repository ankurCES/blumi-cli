import 'package:flutter/material.dart';
import '../state/app.dart';

/// First-run screen: point blugo at the Mac's LAN gateway + log in.
class ConnectScreen extends StatefulWidget {
  final AppController app;
  const ConnectScreen(this.app, {super.key});

  @override
  State<ConnectScreen> createState() => _ConnectScreenState();
}

class _ConnectScreenState extends State<ConnectScreen> {
  final _host = TextEditingController();
  final _port = TextEditingController(text: '7777');
  final _pass = TextEditingController();

  @override
  void dispose() {
    _host.dispose();
    _port.dispose();
    _pass.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final app = widget.app;
    final cs = Theme.of(context).colorScheme;
    return Scaffold(
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 440),
          child: ListView(
            shrinkWrap: true,
            padding: const EdgeInsets.all(24),
            children: [
              Text('✿ blugo',
                  style: TextStyle(
                      fontSize: 44,
                      fontWeight: FontWeight.bold,
                      color: cs.primary)),
              const SizedBox(height: 6),
              Text('connect to your blumi gateway',
                  style: TextStyle(color: cs.onSurface.withValues(alpha: 0.6))),
              const SizedBox(height: 28),
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
                onSubmitted: (_) => _connect(),
              ),
              const SizedBox(height: 22),
              FilledButton(
                onPressed: app.connecting ? null : _connect,
                child: Padding(
                  padding: const EdgeInsets.symmetric(vertical: 6),
                  child: Text(app.connecting ? 'Connecting…' : 'Connect'),
                ),
              ),
              if (app.status.isNotEmpty && !app.connecting)
                Padding(
                  padding: const EdgeInsets.only(top: 14),
                  child: Text(app.status,
                      style: TextStyle(color: cs.error), textAlign: TextAlign.center),
                ),
              const SizedBox(height: 24),
              Text(
                'On your Mac:  blumi serve pair --password <pw>\n'
                'then  blumi serve install',
                style: TextStyle(
                    fontFamily: 'monospace',
                    fontSize: 12,
                    color: cs.onSurface.withValues(alpha: 0.5)),
                textAlign: TextAlign.center,
              ),
            ],
          ),
        ),
      ),
    );
  }

  void _connect() => widget.app.connect(
        host: _host.text.trim(),
        port: int.tryParse(_port.text.trim()) ?? 7777,
        password: _pass.text,
      );
}
