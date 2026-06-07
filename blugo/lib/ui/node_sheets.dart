import 'package:flutter/material.dart';
import '../data/saved_server.dart';
import '../state/app.dart';
import 'kit/kit.dart';

/// Tap a **saved** node → Connect (primary) · Edit · Delete (on the side).
Future<void> showSavedNodeSheet(
    BuildContext context, AppController app, SavedServer server) {
  return showBlumiSheet(
    context,
    title: server.name,
    icon: Icons.dns_outlined,
    child: _SavedNodeActions(app: app, server: server, screen: context),
  );
}

/// Tap a **discovered** node → password-prefilled connect modal (name pre-filled
/// from mDNS, host/port shown read-only, password auto-focused).
Future<void> showDiscoveredNodeSheet(
    BuildContext context, AppController app, SavedServer server) {
  return _showConnectSheet(
    context,
    app,
    title: 'Add ${server.name}',
    icon: Icons.wifi_tethering,
    prefill: server,
    editableEndpoint: false,
    editableName: true,
  );
}

/// Manual add (the "+" action) → a fully editable connect modal.
Future<void> showAddNodeSheet(BuildContext context, AppController app) {
  return _showConnectSheet(
    context,
    app,
    title: 'Add a gateway',
    icon: Icons.add,
    editableEndpoint: true,
    editableName: true,
  );
}

/// Re-auth a saved node whose token went stale (host/port + name fixed).
Future<void> showReauthSheet(
    BuildContext context, AppController app, SavedServer server) {
  return _showConnectSheet(
    context,
    app,
    title: 'Sign in to ${server.name}',
    icon: Icons.lock_outline,
    prefill: server,
    editableEndpoint: false,
    editableName: false,
  );
}

/// Edit a saved node's name/address.
Future<void> showEditNodeSheet(
    BuildContext context, AppController app, SavedServer server) {
  return showBlumiSheet(
    context,
    title: 'Edit ${server.name}',
    icon: Icons.edit_outlined,
    child: _EditForm(app: app, server: server),
  );
}

// ---------------------------------------------------------------------------

class _SavedNodeActions extends StatelessWidget {
  final AppController app;
  final SavedServer server;

  /// The screen context, used to launch follow-up sheets after this one pops.
  final BuildContext screen;
  const _SavedNodeActions(
      {required this.app, required this.server, required this.screen});

  Future<void> _connect(BuildContext sheet) async {
    Navigator.of(sheet).pop();
    if (server.token == null) {
      await showReauthSheet(screen, app, server);
      return;
    }
    await app.connectToSaved(server);
    if (screen.mounted &&
        !app.connected &&
        app.reauthFor?.id == server.id) {
      await showReauthSheet(screen, app, server);
    }
  }

  Future<void> _delete(BuildContext sheet) async {
    final ok = await confirmDialog(
      sheet,
      title: 'Forget ${server.name}?',
      message: 'Removes ${server.endpoint} and its saved token from this phone.',
      confirmLabel: 'Forget',
      danger: true,
      icon: Icons.delete_outline,
    );
    if (ok) {
      await app.removeServer(server.id);
      if (sheet.mounted) Navigator.of(sheet).pop();
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = BlumiTokens.of(context);
    final isCurrent = app.currentServerId == server.id && app.connected;
    return ListenableBuilder(
      listenable: app,
      builder: (context, _) => Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          Row(children: [
            Icon(Icons.lan_outlined, size: 15, color: t.textMuted),
            const SizedBox(width: 6),
            Text(server.endpoint,
                style: TextStyle(color: t.textMuted, fontSize: 13)),
            const Spacer(),
            if (isCurrent) const StatusPill(BlumiStatus.ok, 'connected'),
          ]),
          const SizedBox(height: 16),
          Row(children: [
            Expanded(
              child: GradientButton(
                label: app.connecting ? 'Connecting…' : 'Connect',
                icon: Icons.bolt,
                busy: app.connecting,
                onPressed:
                    app.connecting ? null : () => _connect(context),
              ),
            ),
            const SizedBox(width: 10),
            _SideButton(
              icon: Icons.delete_outline,
              color: t.error,
              tooltip: 'Forget',
              onTap: app.connecting ? null : () => _delete(context),
            ),
          ]),
          const SizedBox(height: 10),
          SizedBox(
            width: double.infinity,
            child: OutlinedButton.icon(
              onPressed: app.connecting
                  ? null
                  : () {
                      Navigator.of(context).pop();
                      showEditNodeSheet(screen, app, server);
                    },
              icon: const Icon(Icons.edit_outlined, size: 18),
              label: const Text('Edit details'),
            ),
          ),
        ],
      ),
    );
  }
}

class _SideButton extends StatelessWidget {
  final IconData icon;
  final Color color;
  final String tooltip;
  final VoidCallback? onTap;
  const _SideButton(
      {required this.icon,
      required this.color,
      required this.tooltip,
      this.onTap});

  @override
  Widget build(BuildContext context) {
    final t = BlumiTokens.of(context);
    return Tooltip(
      message: tooltip,
      child: Material(
        color: color.withValues(alpha: onTap == null ? 0.05 : 0.13),
        borderRadius: BorderRadius.circular(t.radiusSm + 2),
        child: InkWell(
          borderRadius: BorderRadius.circular(t.radiusSm + 2),
          onTap: onTap,
          child: Padding(
            padding: const EdgeInsets.all(13),
            child: Icon(icon, color: color, size: 22),
          ),
        ),
      ),
    );
  }
}

// ---------------------------------------------------------------------------

class _EditForm extends StatefulWidget {
  final AppController app;
  final SavedServer server;
  const _EditForm({required this.app, required this.server});
  @override
  State<_EditForm> createState() => _EditFormState();
}

class _EditFormState extends State<_EditForm> {
  late final _name = TextEditingController(text: widget.server.name);
  late final _host = TextEditingController(text: widget.server.host);
  late final _port =
      TextEditingController(text: widget.server.port.toString());

  @override
  void dispose() {
    _name.dispose();
    _host.dispose();
    _port.dispose();
    super.dispose();
  }

  bool get _endpointChanged =>
      _host.text.trim() != widget.server.host ||
      (int.tryParse(_port.text.trim()) ?? widget.server.port) !=
          widget.server.port;

  Future<void> _save() async {
    await widget.app.editServer(
      widget.server.id,
      name: _name.text,
      host: _host.text.trim(),
      port: int.tryParse(_port.text.trim()) ?? widget.server.port,
    );
    if (mounted) Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final t = BlumiTokens.of(context);
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        TextField(
          controller: _name,
          decoration: const InputDecoration(labelText: 'Name'),
          onChanged: (_) => setState(() {}),
        ),
        const SizedBox(height: 12),
        TextField(
          controller: _host,
          autocorrect: false,
          decoration: const InputDecoration(
              labelText: 'Host', hintText: '10.0.0.61'),
          onChanged: (_) => setState(() {}),
        ),
        const SizedBox(height: 12),
        TextField(
          controller: _port,
          keyboardType: TextInputType.number,
          decoration: const InputDecoration(labelText: 'Port'),
          onChanged: (_) => setState(() {}),
        ),
        if (_endpointChanged) ...[
          const SizedBox(height: 10),
          Row(children: [
            Icon(Icons.info_outline, size: 15, color: t.warning),
            const SizedBox(width: 6),
            Expanded(
              child: Text(
                'Changing the address asks for the password again next connect.',
                style: TextStyle(color: t.warning, fontSize: 12),
              ),
            ),
          ]),
        ],
        const SizedBox(height: 18),
        GradientButton(label: 'Save', icon: Icons.check, onPressed: _save),
      ],
    );
  }
}

// ---------------------------------------------------------------------------

Future<void> _showConnectSheet(
  BuildContext context,
  AppController app, {
  required String title,
  required IconData icon,
  SavedServer? prefill,
  required bool editableEndpoint,
  required bool editableName,
}) {
  return showBlumiSheet(
    context,
    title: title,
    icon: icon,
    child: _ConnectForm(
      app: app,
      prefill: prefill,
      editableEndpoint: editableEndpoint,
      editableName: editableName,
    ),
  );
}

class _ConnectForm extends StatefulWidget {
  final AppController app;
  final SavedServer? prefill;
  final bool editableEndpoint;
  final bool editableName;
  const _ConnectForm({
    required this.app,
    required this.prefill,
    required this.editableEndpoint,
    required this.editableName,
  });
  @override
  State<_ConnectForm> createState() => _ConnectFormState();
}

class _ConnectFormState extends State<_ConnectForm> {
  late final _name = TextEditingController(text: widget.prefill?.name ?? '');
  late final _host = TextEditingController(text: widget.prefill?.host ?? '');
  late final _port =
      TextEditingController(text: (widget.prefill?.port ?? 7777).toString());
  final _pass = TextEditingController();

  @override
  void dispose() {
    _name.dispose();
    _host.dispose();
    _port.dispose();
    _pass.dispose();
    super.dispose();
  }

  Future<void> _connect() async {
    await widget.app.addAndConnect(
      name: _name.text,
      host: _host.text.trim(),
      port: int.tryParse(_port.text.trim()) ?? 7777,
      password: _pass.text,
    );
    if (mounted && widget.app.connected) Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final t = BlumiTokens.of(context);
    final app = widget.app;
    return ListenableBuilder(
      listenable: app,
      builder: (context, _) => Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (!widget.editableEndpoint && widget.prefill != null)
            Padding(
              padding: const EdgeInsets.only(bottom: 12),
              child: Row(children: [
                Icon(Icons.lan_outlined, size: 15, color: t.textMuted),
                const SizedBox(width: 6),
                Text(widget.prefill!.endpoint,
                    style: TextStyle(color: t.textMuted, fontSize: 13)),
              ]),
            ),
          if (widget.editableName) ...[
            TextField(
              controller: _name,
              decoration: const InputDecoration(
                labelText: 'Name (optional)',
                hintText: 'defaults to the machine name',
              ),
            ),
            const SizedBox(height: 12),
          ],
          if (widget.editableEndpoint) ...[
            TextField(
              controller: _host,
              autocorrect: false,
              decoration: const InputDecoration(
                  labelText: 'Host (Mac LAN IP)', hintText: '10.0.0.61'),
            ),
            const SizedBox(height: 12),
            TextField(
              controller: _port,
              keyboardType: TextInputType.number,
              decoration: const InputDecoration(labelText: 'Port'),
            ),
            const SizedBox(height: 12),
          ],
          TextField(
            controller: _pass,
            obscureText: true,
            autofocus: true,
            decoration: const InputDecoration(labelText: 'Password'),
            onSubmitted: (_) => app.connecting ? null : _connect(),
          ),
          if (app.status.isNotEmpty && !app.connecting)
            Padding(
              padding: const EdgeInsets.only(top: 12),
              child: Row(children: [
                Icon(Icons.error_outline, size: 15, color: t.error),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(app.status,
                      style: TextStyle(color: t.error, fontSize: 12.5)),
                ),
              ]),
            ),
          const SizedBox(height: 18),
          GradientButton(
            label: app.connecting ? 'Connecting…' : 'Connect',
            icon: Icons.bolt,
            busy: app.connecting,
            onPressed: app.connecting ? null : _connect,
          ),
          if (widget.editableEndpoint)
            Padding(
              padding: const EdgeInsets.only(top: 14),
              child: Text(
                'On each Mac:  blumi serve pair --password <pw>\n'
                'then  blumi serve install',
                style: TextStyle(
                    fontFamily: 'monospace',
                    fontSize: 11.5,
                    color: t.textMuted.withValues(alpha: 0.8)),
                textAlign: TextAlign.center,
              ),
            ),
        ],
      ),
    );
  }
}
