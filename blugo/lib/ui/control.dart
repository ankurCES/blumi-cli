import 'dart:async';
import 'package:flutter/material.dart';
import '../data/api.dart';
import '../data/cache.dart';
import '../data/elevenlabs.dart';
import '../state/app.dart';
import 'theme.dart';

/// The control center — a draggable sheet mirroring the TUI/web control tabs:
/// Settings (model/persona/theme/yolo), Usage, Skills, Memory.
Future<void> showControlCenter(BuildContext context, AppController app) {
  return showModalBottomSheet(
    context: context,
    isScrollControlled: true,
    showDragHandle: true,
    backgroundColor: Theme.of(context).colorScheme.surface,
    builder: (_) => DraggableScrollableSheet(
      expand: false,
      initialChildSize: 0.7,
      minChildSize: 0.4,
      maxChildSize: 0.95,
      builder: (context, scroll) => _ControlCenter(app, scroll),
    ),
  );
}

class _ControlCenter extends StatelessWidget {
  final AppController app;
  final ScrollController scroll;
  const _ControlCenter(this.app, this.scroll);

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return DefaultTabController(
      length: 6,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 4, 16, 0),
            child: Row(children: [
              Text('✿ control center',
                  style: TextStyle(
                      fontWeight: FontWeight.bold, color: cs.primary, fontSize: 16)),
            ]),
          ),
          TabBar(
            isScrollable: true,
            tabAlignment: TabAlignment.start,
            labelColor: cs.primary,
            indicatorColor: cs.primary,
            tabs: const [
              Tab(text: 'Settings'),
              Tab(text: 'Status'),
              Tab(text: 'Tasks'),
              Tab(text: 'Usage'),
              Tab(text: 'Skills'),
              Tab(text: 'Memory'),
            ],
          ),
          Expanded(
            child: TabBarView(children: [
              _SettingsTab(app, scroll),
              _StatusTab(app, scroll),
              _TasksTab(app, scroll),
              _UsageTab(app, scroll),
              _SkillsTab(app, scroll),
              _MemoryTab(app, scroll),
            ]),
          ),
        ],
      ),
    );
  }
}

// --- async helper (loading / retry-on-error / content) ---------------------

/// Stale-while-revalidate view: paints cached data instantly (a thin progress
/// bar shows while revalidating), only hits the network when the entry is
/// stale, and shows Retry on error when there's nothing cached.
class _AsyncView<T> extends StatefulWidget {
  final DataCache cache;
  final String cacheKey;
  final Duration ttl;
  final Future<dynamic> Function() fetch; // raw JSON, cached as-is
  final T Function(dynamic raw) parse;
  final Widget Function(BuildContext, T, Future<void> Function()) builder;
  const _AsyncView({
    required this.cache,
    required this.cacheKey,
    required this.ttl,
    required this.fetch,
    required this.parse,
    required this.builder,
  });
  @override
  State<_AsyncView<T>> createState() => _AsyncViewState<T>();
}

class _AsyncViewState<T> extends State<_AsyncView<T>> {
  T? _data;
  Object? _error;
  bool _loading = false;

  @override
  void initState() {
    super.initState();
    final cached = widget.cache.peek(widget.cacheKey);
    if (cached != null) {
      try {
        _data = widget.parse(cached);
      } catch (_) {}
    }
    if (_data == null || !widget.cache.isFresh(widget.cacheKey, widget.ttl)) {
      WidgetsBinding.instance.addPostFrameCallback((_) => _refresh());
    }
  }

  Future<void> _refresh() async {
    if (_loading) return;
    if (mounted) setState(() => _loading = true);
    try {
      final raw = await widget.fetch();
      widget.cache.put(widget.cacheKey, raw);
      final parsed = widget.parse(raw);
      if (mounted) {
        setState(() {
          _data = parsed;
          _error = null;
        });
      }
    } catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    if (_data != null) {
      return Stack(children: [
        Positioned.fill(
          child: RefreshIndicator(
            onRefresh: _refresh, // pull down to force-refresh (bypass TTL)
            child: widget.builder(context, _data as T, _refresh),
          ),
        ),
        if (_loading)
          const Positioned(
              top: 0,
              left: 0,
              right: 0,
              child: LinearProgressIndicator(minHeight: 2)),
      ]);
    }
    if (_error != null) return _errorRetry(cs, _refresh);
    return const Center(child: CircularProgressIndicator());
  }
}

Widget _errorRetry(ColorScheme cs, Future<void> Function() onRetry) => Center(
      child: Column(mainAxisSize: MainAxisSize.min, children: [
        Icon(Icons.cloud_off, color: cs.onSurface.withValues(alpha: 0.4)),
        const SizedBox(height: 8),
        Text('couldn’t load',
            style: TextStyle(color: cs.onSurface.withValues(alpha: 0.6))),
        const SizedBox(height: 8),
        OutlinedButton.icon(
            onPressed: onRetry,
            icon: const Icon(Icons.refresh),
            label: const Text('Retry')),
      ]),
    );

// --- Settings --------------------------------------------------------------

class _SettingsTab extends StatefulWidget {
  final AppController app;
  final ScrollController scroll;
  const _SettingsTab(this.app, this.scroll);
  @override
  State<_SettingsTab> createState() => _SettingsTabState();
}

class _SettingsTabState extends State<_SettingsTab> {
  // Voice config (loaded from /api/settings; key fields are write-only).
  final _ttsKey = TextEditingController();
  final _ttsVoice = TextEditingController();
  final _sttKey = TextEditingController();
  String _ttsProvider = 'elevenlabs';
  bool _voiceEnabled = false;
  bool _ttsKeySet = false;

  // ElevenLabs voice picker — populated by authenticating with the entered key.
  List<VoiceOption> _voices = [];
  bool _loadingVoices = false;
  String? _voiceError;

  ApiClient get _api => widget.app.session!.api;

  @override
  void initState() {
    super.initState();
    widget.app.loadMeta(); // SWR: paints from cache, revalidates if stale
    _loadVoice();
  }

  @override
  void dispose() {
    _ttsKey.dispose();
    _ttsVoice.dispose();
    _sttKey.dispose();
    super.dispose();
  }

  Future<void> _loadVoice() async {
    try {
      final v = (await _api.settings())['voice'] as Map? ?? {};
      if (!mounted) return;
      setState(() {
        _voiceEnabled = v['enabled'] as bool? ?? false;
        final p = v['tts_provider']?.toString() ?? '';
        if (p.isNotEmpty) _ttsProvider = p;
        _ttsVoice.text = v['tts_voice']?.toString() ?? '';
        _ttsKeySet = v['tts_api_key_set'] as bool? ?? false;
      });
    } catch (_) {}
  }

  /// Authenticate the entered ElevenLabs key and load its voices into the
  /// dropdown. The key is write-only on the gateway, so we use what the user
  /// just typed — re-enter it to (re)load.
  Future<void> _loadVoices() async {
    final key = _ttsKey.text.trim();
    if (key.isEmpty) {
      setState(() => _voiceError = _ttsKeySet
          ? 'Re-enter your API key to load voices'
          : 'Enter your API key first');
      return;
    }
    setState(() {
      _loadingVoices = true;
      _voiceError = null;
    });
    try {
      final list = await fetchElevenLabsVoices(key);
      if (!mounted) return;
      setState(() {
        _voices = list;
        _loadingVoices = false;
        if (list.isEmpty) {
          _voiceError = 'no voices on this account';
        } else if (!list.any((v) => v.id == _ttsVoice.text.trim())) {
          _ttsVoice.text = list.first.id; // default to the first voice
        }
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _loadingVoices = false;
        _voiceError = '$e';
      });
    }
  }

  Future<void> _saveVoice() async {
    final messenger = ScaffoldMessenger.of(context);
    final patch = <String, dynamic>{
      'voice_enabled': _voiceEnabled,
      'tts_provider': _ttsProvider,
      if (_ttsVoice.text.trim().isNotEmpty) 'tts_voice': _ttsVoice.text.trim(),
      if (_ttsKey.text.isNotEmpty) 'tts_api_key': _ttsKey.text,
      if (_ttsProvider == 'elevenlabs') 'tts_model': 'eleven_multilingual_v2',
    };
    // A mic-STT key also needs an OpenAI-compatible Whisper endpoint.
    if (_sttKey.text.isNotEmpty) {
      patch['voice_api_key'] = _sttKey.text;
      patch['stt_base_url'] = 'https://api.openai.com/v1';
      patch['stt_model'] = 'whisper-1';
    }
    try {
      await _api.setSettings(patch);
      _ttsKey.clear();
      _sttKey.clear();
      await _loadVoice();
      messenger.showSnackBar(const SnackBar(content: Text('voice settings saved')));
    } catch (e) {
      messenger.showSnackBar(SnackBar(content: Text('save failed: $e')));
    }
  }

  @override
  Widget build(BuildContext context) {
    final app = widget.app;
    final cs = Theme.of(context).colorScheme;
    return AnimatedBuilder(
      animation: app,
      builder: (context, _) => ListView(
        controller: widget.scroll,
        padding: const EdgeInsets.all(16),
        children: [
          _label('Theme', cs),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              for (final t in blumiThemes)
                ChoiceChip(
                  label: Text(t.name),
                  selected: app.themeName == t.name,
                  avatar: CircleAvatar(backgroundColor: t.primary, radius: 7),
                  onSelected: (_) => app.setTheme(t.name),
                ),
            ],
          ),
          const SizedBox(height: 18),
          _label('Model', cs),
          Builder(builder: (context) {
            final models = app.models;
            final current = app.session?.modelName ?? '';
            if (models.isEmpty) {
              return Text(current.isEmpty ? '(loading…)' : current,
                  style: TextStyle(color: cs.onSurface.withValues(alpha: 0.7)));
            }
            return Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                for (final m in models)
                  ChoiceChip(
                    label: Text(m),
                    selected: current == m,
                    onSelected: (_) {
                      app.session?.applyModel(m); // optimistic
                      _api.setModel(m);
                    },
                  ),
              ],
            );
          }),
          const SizedBox(height: 18),
          _label('Persona', cs),
          if (app.personas.isEmpty)
            const Text('(loading…)')
          else
            Column(
              children: [
                for (final p in app.personas)
                  ListTile(
                    dense: true,
                    contentPadding: EdgeInsets.zero,
                    selected: app.activePersona == p.name,
                    leading: Icon(app.activePersona == p.name
                        ? Icons.radio_button_checked
                        : Icons.radio_button_unchecked),
                    title: Text(p.name),
                    subtitle: p.description.isEmpty
                        ? null
                        : Text(p.description,
                            maxLines: 2, overflow: TextOverflow.ellipsis),
                    onTap: () => app.setPersona(p.name), // optimistic in AppController
                  ),
              ],
            ),
          const SizedBox(height: 8),
          SwitchListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text('YOLO — auto-approve tools'),
            subtitle: Text('skip permission cards',
                style: TextStyle(color: cs.onSurface.withValues(alpha: 0.6))),
            value: app.yolo,
            onChanged: app.setYolo,
          ),
          const Divider(height: 28),
          _label('Voice', cs),
          SwitchListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text('Enable voice'),
            value: _voiceEnabled,
            onChanged: (v) => setState(() => _voiceEnabled = v),
          ),
          Wrap(spacing: 8, children: [
            for (final p in const ['elevenlabs', 'openai'])
              ChoiceChip(
                label: Text(p),
                selected: _ttsProvider == p,
                onSelected: (_) => setState(() {
                  _ttsProvider = p;
                  _voices = [];
                  _voiceError = null;
                }),
              ),
          ]),
          const SizedBox(height: 12),
          TextField(
            controller: _ttsKey,
            obscureText: true,
            onSubmitted:
                _ttsProvider == 'elevenlabs' ? (_) => _loadVoices() : null,
            decoration: InputDecoration(
              labelText: _ttsProvider == 'elevenlabs'
                  ? 'ElevenLabs API key'
                  : 'TTS API key',
              hintText: _ttsKeySet ? 'saved ✓ — blank keeps current' : null,
              border: const OutlineInputBorder(),
            ),
          ),
          // ElevenLabs: authenticate the key and pick the voice from a dropdown.
          if (_ttsProvider == 'elevenlabs') ...[
            const SizedBox(height: 10),
            Row(children: [
              OutlinedButton.icon(
                onPressed: _loadingVoices ? null : _loadVoices,
                icon: _loadingVoices
                    ? const SizedBox(
                        width: 16,
                        height: 16,
                        child: CircularProgressIndicator(strokeWidth: 2))
                    : const Icon(Icons.podcasts, size: 18),
                label: Text(_voices.isEmpty
                    ? 'Authenticate & load voices'
                    : 'Reload voices'),
              ),
              const SizedBox(width: 10),
              if (_voices.isNotEmpty)
                Text('✓ ${_voices.length} voices',
                    style: const TextStyle(
                        color: Color(0xFF4FE0A0),
                        fontWeight: FontWeight.w600)),
            ]),
            if (_voiceError != null)
              Padding(
                padding: const EdgeInsets.only(top: 6),
                child: Text(_voiceError!,
                    style: TextStyle(color: cs.error, fontSize: 12.5)),
              ),
            const SizedBox(height: 12),
            if (_voices.isNotEmpty)
              DropdownButtonFormField<String>(
                initialValue:
                    _voices.any((v) => v.id == _ttsVoice.text.trim())
                        ? _ttsVoice.text.trim()
                        : null,
                isExpanded: true,
                decoration: const InputDecoration(
                  labelText: 'Voice',
                  border: OutlineInputBorder(),
                ),
                items: [
                  for (final v in _voices)
                    DropdownMenuItem(
                        value: v.id,
                        child: Text(v.name, overflow: TextOverflow.ellipsis)),
                ],
                onChanged: (id) =>
                    setState(() => _ttsVoice.text = id ?? ''),
              )
            else
              TextField(
                controller: _ttsVoice,
                decoration: const InputDecoration(
                  labelText: 'Voice ID (or load voices above)',
                  border: OutlineInputBorder(),
                ),
              ),
          ] else ...[
            const SizedBox(height: 12),
            TextField(
              controller: _ttsVoice,
              decoration: const InputDecoration(
                labelText: 'Voice (e.g. alloy)',
                border: OutlineInputBorder(),
              ),
            ),
          ],
          const SizedBox(height: 12),
          TextField(
            controller: _sttKey,
            obscureText: true,
            decoration: const InputDecoration(
              labelText: 'Mic key (OpenAI Whisper, optional)',
              hintText: 'for speech-to-text input',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 14),
          FilledButton(
            onPressed: _saveVoice,
            child: const Padding(
              padding: EdgeInsets.symmetric(vertical: 6),
              child: Text('Save voice settings'),
            ),
          ),
          const Divider(height: 28),
          _label('Maintenance', cs),
          Wrap(spacing: 8, runSpacing: 8, children: [
            OutlinedButton.icon(
              onPressed: _reload,
              icon: const Icon(Icons.refresh, size: 18),
              label: const Text('Reload agent'),
            ),
            OutlinedButton.icon(
              onPressed: _recover,
              icon: const Icon(Icons.healing, size: 18),
              label: const Text('Recover'),
            ),
            OutlinedButton.icon(
              onPressed: _restart,
              icon: Icon(Icons.power_settings_new, size: 18, color: cs.error),
              label: Text('Restart gateway', style: TextStyle(color: cs.error)),
            ),
            OutlinedButton.icon(
              onPressed: _editConfig,
              icon: const Icon(Icons.tune, size: 18),
              label: const Text('Edit config'),
            ),
            OutlinedButton.icon(
              onPressed: _newSkill,
              icon: const Icon(Icons.auto_awesome, size: 18),
              label: const Text('New skill'),
            ),
          ]),
          const SizedBox(height: 18),
          _label('Grid peers', cs),
          FutureBuilder<(List<GridPeer>, Map<String, dynamic>)>(
            future: _api.gridPeers(),
            builder: (context, snap) {
              if (snap.connectionState != ConnectionState.done) {
                return const Padding(
                    padding: EdgeInsets.all(8), child: LinearProgressIndicator());
              }
              if (snap.hasError) {
                return Text('grid off / unavailable',
                    style:
                        TextStyle(color: cs.onSurface.withValues(alpha: 0.6)));
              }
              final peers = snap.data!.$1;
              if (peers.isEmpty) {
                return Text('no peers discovered on the network',
                    style:
                        TextStyle(color: cs.onSurface.withValues(alpha: 0.6)));
              }
              return Column(
                children: [
                  for (final p in peers)
                    ListTile(
                      dense: true,
                      contentPadding: EdgeInsets.zero,
                      leading: Icon(Icons.dns,
                          color: p.online
                              ? Colors.greenAccent
                              : cs.onSurface.withValues(alpha: 0.4)),
                      title: Text(p.name),
                      subtitle: Text('${p.host}:${p.port} · v${p.version}'),
                      trailing: Text(p.online ? 'online' : 'offline',
                          style: TextStyle(
                              fontSize: 11,
                              color: p.online
                                  ? Colors.greenAccent
                                  : cs.onSurface.withValues(alpha: 0.5))),
                    ),
                ],
              );
            },
          ),
          const SizedBox(height: 8),
        ],
      ),
    );
  }

  // --- Self-management actions ---

  Future<void> _reload() async {
    final m = ScaffoldMessenger.of(context);
    try {
      await _api.selfReload();
      m.showSnackBar(const SnackBar(content: Text('reloading agent…')));
    } catch (e) {
      m.showSnackBar(SnackBar(content: Text('reload failed: $e')));
    }
  }

  Future<void> _recover() async {
    final m = ScaffoldMessenger.of(context);
    try {
      final r = await _api.selfRecover();
      m.showSnackBar(
          SnackBar(content: Text('recover: ${r['action'] ?? r['error'] ?? 'ok'}')));
    } catch (e) {
      m.showSnackBar(SnackBar(content: Text('recover failed: $e')));
    }
  }

  Future<void> _restart() async {
    final m = ScaffoldMessenger.of(context);
    final ok = await showDialog<bool>(
      context: context,
      builder: (c) => AlertDialog(
        title: const Text('Restart gateway?'),
        content: const Text(
            'Bounces the gateway service. In-flight turns are interrupted; '
            'the app reconnects automatically.'),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(c, false),
              child: const Text('Cancel')),
          FilledButton(
              onPressed: () => Navigator.pop(c, true),
              child: const Text('Restart')),
        ],
      ),
    );
    if (ok != true) return;
    try {
      final r = await _api.selfRestart();
      m.showSnackBar(SnackBar(
          content: Text('restart: ${r['mode'] ?? (r['error'] ?? 'ok')}')));
    } catch (e) {
      m.showSnackBar(SnackBar(content: Text('restart failed: $e')));
    }
  }

  Future<void> _editConfig() async {
    final keyC = TextEditingController();
    final valC = TextEditingController();
    final m = ScaffoldMessenger.of(context);
    final ok = await showDialog<bool>(
      context: context,
      builder: (c) => AlertDialog(
        title: const Text('Set config key'),
        content: Column(mainAxisSize: MainAxisSize.min, children: [
          TextField(
              controller: keyC,
              decoration: const InputDecoration(
                  labelText: 'dotted key, e.g. llm.temperature')),
          TextField(
              controller: valC,
              decoration:
                  const InputDecoration(labelText: 'value (JSON or text)')),
        ]),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(c, false),
              child: const Text('Cancel')),
          FilledButton(
              onPressed: () => Navigator.pop(c, true),
              child: const Text('Set + reload')),
        ],
      ),
    );
    if (ok != true || keyC.text.trim().isEmpty) return;
    try {
      final r =
          await _api.selfConfigSet(keyC.text.trim(), valC.text, reload: true);
      m.showSnackBar(SnackBar(
          content: Text(r['ok'] == true
              ? (r['message']?.toString() ?? 'config set')
              : 'error: ${r['error']}')));
    } catch (e) {
      m.showSnackBar(SnackBar(content: Text('config failed: $e')));
    }
  }

  Future<void> _newSkill() async {
    final nameC = TextEditingController();
    final descC = TextEditingController();
    final instrC = TextEditingController();
    final m = ScaffoldMessenger.of(context);
    final ok = await showDialog<bool>(
      context: context,
      builder: (c) => AlertDialog(
        title: const Text('New / update skill'),
        content: Column(mainAxisSize: MainAxisSize.min, children: [
          TextField(
              controller: nameC,
              decoration: const InputDecoration(labelText: 'name (slug)')),
          TextField(
              controller: descC,
              decoration: const InputDecoration(labelText: 'description')),
          TextField(
              controller: instrC,
              maxLines: 4,
              decoration:
                  const InputDecoration(labelText: 'instructions (markdown)')),
        ]),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(c, false),
              child: const Text('Cancel')),
          FilledButton(
              onPressed: () => Navigator.pop(c, true),
              child: const Text('Save + reload')),
        ],
      ),
    );
    if (ok != true || nameC.text.trim().isEmpty) return;
    try {
      final r = await _api.skillWrite(
          nameC.text.trim(), descC.text.trim(), instrC.text,
          reload: true);
      m.showSnackBar(SnackBar(
          content: Text(r['ok'] == true ? 'skill saved' : 'error: ${r['error']}')));
    } catch (e) {
      m.showSnackBar(SnackBar(content: Text('skill failed: $e')));
    }
  }
}

// --- Status (uptime + live run metrics) ------------------------------------

class _StatusTab extends StatefulWidget {
  final AppController app;
  final ScrollController scroll;
  const _StatusTab(this.app, this.scroll);
  @override
  State<_StatusTab> createState() => _StatusTabState();
}

class _StatusTabState extends State<_StatusTab> {
  late Future<Map<String, dynamic>> _status;

  @override
  void initState() {
    super.initState();
    _status = widget.app.session!.api.status();
  }

  Future<void> _reload() {
    final f = widget.app.session!.api.status();
    setState(() => _status = f);
    return f.then((_) {}).catchError((_) {});
  }

  String _fmtUptime(num secs) {
    final s = secs.toInt();
    final h = s ~/ 3600, m = (s % 3600) ~/ 60, sec = s % 60;
    if (h > 0) return '${h}h ${m}m';
    if (m > 0) return '${m}m ${sec}s';
    return '${sec}s';
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return FutureBuilder<Map<String, dynamic>>(
      future: _status,
      builder: (context, snap) {
        if (snap.connectionState == ConnectionState.waiting) {
          return const Center(child: CircularProgressIndicator());
        }
        if (snap.hasError || !snap.hasData) {
          return _errorRetry(cs, _reload);
        }
        final st = snap.data!;
        final s = widget.app.session!;
        return AnimatedBuilder(
          animation: s,
          builder: (context, _) => RefreshIndicator(
            onRefresh: () async {
              final f = widget.app.session!.api.status();
              setState(() => _status = f);
              await f;
            },
            child: ListView(
              controller: widget.scroll,
              padding: const EdgeInsets.all(16),
              children: [
                _row('uptime', _fmtUptime((st['uptime_secs'] as num?) ?? 0), cs),
                _row('model', st['model']?.toString() ?? s.modelName, cs),
                _row('version', st['version']?.toString() ?? '—', cs),
                const SizedBox(height: 14),
                _label('context', cs),
                ClipRRect(
                  borderRadius: BorderRadius.circular(4),
                  child: LinearProgressIndicator(value: s.contextFrac, minHeight: 8),
                ),
                const SizedBox(height: 2),
                Text('${(s.contextFrac * 100).round()}%',
                    style: const TextStyle(fontSize: 12)),
                const SizedBox(height: 14),
                _row('tokens', '↑${s.inputTokens}  ↓${s.outputTokens}', cs),
                if (s.costUsd > 0)
                  _row('cost', '\$${s.costUsd.toStringAsFixed(4)}', cs),
                const SizedBox(height: 14),
                _label('working dir', cs),
                Text(st['working_dir']?.toString() ?? '—',
                    style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
              ],
            ),
          ),
        );
      },
    );
  }

  Widget _row(String k, String v, ColorScheme cs) => Padding(
        padding: const EdgeInsets.symmetric(vertical: 4),
        child: Row(
          mainAxisAlignment: MainAxisAlignment.spaceBetween,
          children: [
            Text(k, style: TextStyle(color: cs.onSurface.withValues(alpha: 0.7))),
            Flexible(
              child: Text(v,
                  textAlign: TextAlign.right,
                  style: const TextStyle(fontFamily: 'monospace', fontSize: 13)),
            ),
          ],
        ),
      );
}

// --- Tasks (the persistent board) ------------------------------------------

class _TasksTab extends StatefulWidget {
  final AppController app;
  final ScrollController scroll;
  const _TasksTab(this.app, this.scroll);
  @override
  State<_TasksTab> createState() => _TasksTabState();
}

class _TasksTabState extends State<_TasksTab> {
  late Future<List<TaskItem>> _tasks;
  Map<String, dynamic> _loop = const {};
  Timer? _timer;

  ApiClient get _api => widget.app.session!.api;

  @override
  void initState() {
    super.initState();
    _tasks = _api.tasks();
    _refreshLoop();
    // While the loop runs, keep the board + status fresh.
    _timer = Timer.periodic(const Duration(seconds: 2), (_) {
      if ((_loop['running'] as bool?) ?? false) {
        if (mounted) setState(() => _tasks = _api.tasks());
        _refreshLoop();
      }
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  Future<void> _refreshLoop() async {
    try {
      final l = await _api.loopStatus();
      if (mounted) setState(() => _loop = l);
    } catch (_) {}
  }

  Future<void> _toggleLoop() async {
    final running = (_loop['running'] as bool?) ?? false;
    setState(() => _loop = {..._loop, 'running': !running}); // optimistic
    try {
      running ? await _api.loopStop() : await _api.loopStart();
    } catch (_) {}
    await _refreshLoop();
    if (mounted) setState(() => _tasks = _api.tasks());
  }

  // Display order + glyph/colour per state.
  static const _order = ['doing', 'review', 'todo', 'done', 'cancelled'];

  (String, Color) _style(String state, ColorScheme cs) => switch (state) {
        'doing' => ('▶', cs.primary),
        'review' => ('→', cs.secondary),
        'done' => ('✓', Colors.greenAccent),
        'cancelled' => ('✗', cs.onSurface.withValues(alpha: 0.4)),
        _ => ('○', cs.onSurface.withValues(alpha: 0.7)),
      };

  Widget _loopBar(ColorScheme cs) {
    final running = (_loop['running'] as bool?) ?? false;
    final iter = (_loop['iter'] as num?)?.toInt() ?? 0;
    final current = _loop['current']?.toString() ?? '';
    return Container(
      padding: const EdgeInsets.fromLTRB(16, 8, 12, 8),
      child: Row(
        children: [
          Expanded(
            child: Text(
              running
                  ? 'loop running · iter $iter${current.isEmpty ? '' : ' · $current'}'
                  : 'loop idle',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(
                  fontSize: 12,
                  color: running ? cs.primary : cs.onSurface.withValues(alpha: 0.6)),
            ),
          ),
          const SizedBox(width: 8),
          running
              ? FilledButton.tonalIcon(
                  onPressed: _toggleLoop,
                  icon: const Icon(Icons.stop, size: 18),
                  label: const Text('Stop'))
              : FilledButton.icon(
                  onPressed: _toggleLoop,
                  icon: const Icon(Icons.play_arrow, size: 18),
                  label: const Text('Run loop')),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Column(
      children: [
        _loopBar(cs),
        const Divider(height: 1),
        Expanded(child: _board(cs)),
      ],
    );
  }

  Widget _board(ColorScheme cs) {
    return FutureBuilder<List<TaskItem>>(
      future: _tasks,
      builder: (context, snap) {
        if (snap.connectionState == ConnectionState.waiting) {
          return const Center(child: CircularProgressIndicator());
        }
        if (snap.hasError || !snap.hasData) {
          return _errorRetry(cs, () {
            final f = _api.tasks();
            setState(() => _tasks = f);
            return f.then((_) {}).catchError((_) {});
          });
        }
        final tasks = snap.data!;
        if (tasks.isEmpty) {
          return Center(
              child: Text('(no tasks — add with `blumi task add`)',
                  style: TextStyle(color: cs.onSurface.withValues(alpha: 0.6))));
        }
        final byState = {for (final s in _order) s: <TaskItem>[]};
        for (final t in tasks) {
          (byState[t.state] ?? byState['todo']!).add(t);
        }
        return RefreshIndicator(
          onRefresh: () async {
            final f = widget.app.session!.api.tasks();
            setState(() => _tasks = f);
            await f;
          },
          child: ListView(
            controller: widget.scroll,
            padding: const EdgeInsets.all(16),
            children: [
              for (final state in _order)
                if (byState[state]!.isNotEmpty) ...[
                  Padding(
                    padding: const EdgeInsets.only(top: 8, bottom: 4),
                    child: Text('${state.toUpperCase()} · ${byState[state]!.length}',
                        style: TextStyle(
                            fontWeight: FontWeight.bold,
                            fontSize: 12,
                            color: cs.onSurface.withValues(alpha: 0.7))),
                  ),
                  for (final t in byState[state]!) _taskTile(t, cs),
                ],
            ],
          ),
        );
      },
    );
  }

  Widget _taskTile(TaskItem t, ColorScheme cs) {
    final (glyph, color) = _style(t.state, cs);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('$glyph ', style: TextStyle(color: color)),
          Container(
            margin: const EdgeInsets.only(right: 8, top: 1),
            padding: const EdgeInsets.symmetric(horizontal: 5, vertical: 1),
            decoration: BoxDecoration(
              color: cs.onSurface.withValues(alpha: 0.1),
              borderRadius: BorderRadius.circular(4),
            ),
            child: Text('P${t.priority}',
                style: TextStyle(fontSize: 10, color: cs.onSurface.withValues(alpha: 0.7))),
          ),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(t.title,
                    style: TextStyle(
                        decoration: t.state == 'done' || t.state == 'cancelled'
                            ? TextDecoration.lineThrough
                            : null)),
                if (t.detail.isNotEmpty)
                  Text(t.detail,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                          fontSize: 12, color: cs.onSurface.withValues(alpha: 0.6))),
                // Remote-runtime attribution: which grid peer is executing this.
                if (t.owner != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 2),
                    child: Row(mainAxisSize: MainAxisSize.min, children: [
                      Icon(Icons.dns, size: 12, color: cs.tertiary),
                      const SizedBox(width: 3),
                      Text('remote · ${t.owner}',
                          style: TextStyle(
                              fontSize: 11,
                              color: cs.tertiary,
                              fontWeight: FontWeight.w500)),
                    ]),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

// --- Usage -----------------------------------------------------------------

class _UsageTab extends StatelessWidget {
  final AppController app;
  final ScrollController scroll;
  const _UsageTab(this.app, this.scroll);
  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return _AsyncView<Map<String, dynamic>>(
      cache: app.cache,
      cacheKey: app.ck('usage'),
      ttl: const Duration(seconds: 20),
      fetch: () => app.session!.api.getJson('/api/usage'),
      parse: (raw) =>
          ((raw as Map)['usage'] as Map?)?.cast<String, dynamic>() ?? {},
      builder: (context, u, refresh) {
        if (u.isEmpty) {
          return Center(
              child: Text('(no usage yet)',
                  style: TextStyle(color: cs.onSurface.withValues(alpha: 0.6))));
        }
        return ListView(
          controller: scroll,
          padding: const EdgeInsets.all(16),
          children: [
            for (final e in u.entries)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 4),
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    Text(e.key, style: const TextStyle(fontSize: 13)),
                    Text('${e.value}',
                        style: const TextStyle(
                            fontSize: 13, fontFamily: 'monospace')),
                  ],
                ),
              ),
          ],
        );
      },
    );
  }
}

// --- Skills ----------------------------------------------------------------

class _SkillsTab extends StatelessWidget {
  final AppController app;
  final ScrollController scroll;
  const _SkillsTab(this.app, this.scroll);
  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return _AsyncView<List<String>>(
      cache: app.cache,
      cacheKey: app.ck('skills'),
      ttl: const Duration(minutes: 10),
      fetch: () => app.session!.api.getJson('/api/skills'),
      parse: (raw) => (((raw as Map)['skills'] as List?) ?? [])
          .map((s) => s is Map ? (s['name'] ?? '$s').toString() : '$s')
          .toList(),
      builder: (context, skills, refresh) {
        if (skills.isEmpty) {
          return Center(
              child: Text('(no skills)',
                  style: TextStyle(color: cs.onSurface.withValues(alpha: 0.6))));
        }
        return ListView(
          controller: scroll,
          padding: const EdgeInsets.all(16),
          children: [
            for (final s in skills)
              ListTile(
                dense: true,
                contentPadding: EdgeInsets.zero,
                leading: Icon(Icons.auto_awesome, size: 18, color: cs.secondary),
                title: Text(s),
              ),
          ],
        );
      },
    );
  }
}

// --- Memory ----------------------------------------------------------------

class _MemoryTab extends StatelessWidget {
  final AppController app;
  final ScrollController scroll;
  const _MemoryTab(this.app, this.scroll);
  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return _AsyncView<(String, String)>(
      cache: app.cache,
      cacheKey: app.ck('memory'),
      ttl: const Duration(seconds: 60),
      fetch: () => app.session!.api.getJson('/api/memory'),
      parse: (raw) => (
        (raw as Map)['memory']?.toString() ?? '',
        raw['user']?.toString() ?? '',
      ),
      builder: (context, mem, refresh) {
        final (project, user) = mem;
        return ListView(
          controller: scroll,
          padding: const EdgeInsets.all(16),
          children: [
            _label('project memory', cs),
            _memoryBlock(project.isEmpty ? '(empty)' : project, cs),
            const SizedBox(height: 16),
            _label('user memory', cs),
            _memoryBlock(user.isEmpty ? '(empty)' : user, cs),
          ],
        );
      },
    );
  }

  Widget _memoryBlock(String text, ColorScheme cs) => Container(
        width: double.infinity,
        margin: const EdgeInsets.only(top: 6),
        padding: const EdgeInsets.all(10),
        decoration: BoxDecoration(
          color: Colors.black.withValues(alpha: 0.25),
          borderRadius: BorderRadius.circular(8),
        ),
        child: SelectableText(text,
            style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
      );
}

Widget _label(String t, ColorScheme cs) => Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Text(t,
          style: TextStyle(
              fontWeight: FontWeight.bold,
              color: cs.onSurface.withValues(alpha: 0.7),
              fontSize: 13)),
    );
