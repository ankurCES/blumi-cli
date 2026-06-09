import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:webview_flutter/webview_flutter.dart';
import '../data/api.dart';
import '../data/cache.dart';
import '../data/elevenlabs.dart';
import '../state/app.dart';
import 'kit/kit.dart';
import 'theme.dart';

/// Open the control center as a full screen of its own (pushed route).
Future<void> showControlCenter(BuildContext context, AppController app) {
  return Navigator.of(context).push(
    MaterialPageRoute<void>(builder: (_) => ControlCenterScreen(app)),
  );
}

/// The control center as a standalone screen — a tabbed Scaffold mirroring the
/// TUI/web control tabs (Agent · Work · Grid · Knowledge). Each tab gets its
/// own ScrollController (one controller can't attach to multiple TabBarView
/// children), and the Scaffold lifts focused fields above the soft keyboard.
class ControlCenterScreen extends StatefulWidget {
  final AppController app;
  const ControlCenterScreen(this.app, {super.key});
  @override
  State<ControlCenterScreen> createState() => _ControlCenterScreenState();
}

class _ControlCenterScreenState extends State<ControlCenterScreen> {
  static const _tabCount = 12;
  final List<ScrollController> _scrolls =
      List.generate(_tabCount, (_) => ScrollController());

  @override
  void dispose() {
    for (final c in _scrolls) {
      c.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final app = widget.app;
    return DefaultTabController(
      length: _tabCount,
      child: Scaffold(
        appBar: AppBar(
          titleSpacing: 12,
          title: const GradientText(
            '✿ control center',
            style: TextStyle(fontWeight: FontWeight.w800, fontSize: 18),
          ),
          bottom: TabBar(
            isScrollable: true,
            tabAlignment: TabAlignment.start,
            labelColor: cs.primary,
            indicatorColor: cs.primary,
            // Tabs ordered by cluster: Agent · Work · Grid · Knowledge.
            tabs: const [
              // Agent
              _IconTab(Icons.tune, 'Settings'),
              _IconTab(Icons.monitor_heart_outlined, 'Status'),
              _IconTab(Icons.bar_chart, 'Usage'),
              // Work
              _IconTab(Icons.checklist, 'Tasks'),
              _IconTab(Icons.assignment_outlined, 'Plans'),
              _IconTab(Icons.healing, 'Heal'),
              // Grid
              _IconTab(Icons.hub_outlined, 'Grid'),
              // Knowledge
              _IconTab(Icons.auto_awesome, 'Skills'),
              _IconTab(Icons.psychology_outlined, 'Memory'),
              _IconTab(Icons.code, 'Code'),
              _IconTab(Icons.account_tree_outlined, 'Graph'),
              _IconTab(Icons.history, 'Retro'),
            ],
          ),
        ),
        body: TabBarView(
          children: [
            _SettingsTab(app, _scrolls[0]),
            _StatusTab(app, _scrolls[1]),
            _UsageTab(app, _scrolls[2]),
            _TasksTab(app, _scrolls[3]),
            _PlansTab(app, _scrolls[4]),
            _HealTab(app, _scrolls[5]),
            _GridTab(app, _scrolls[6]),
            _SkillsTab(app, _scrolls[7]),
            _MemoryTab(app, _scrolls[8]),
            _KnowledgeTab(app, _scrolls[9]),
            _GraphTab(app, _scrolls[10]),
            _RetroTab(app, _scrolls[11]),
          ],
        ),
      ),
    );
  }
}

/// A compact tab with a leading icon (icon beside label, single-row height).
class _IconTab extends StatelessWidget {
  final IconData icon;
  final String label;
  const _IconTab(this.icon, this.label);
  @override
  Widget build(BuildContext context) => Tab(
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 16),
            const SizedBox(width: 6),
            Text(label),
          ],
        ),
      );
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
      return Stack(
        children: [
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
              child: LinearProgressIndicator(minHeight: 2),
            ),
        ],
      );
    }
    if (_error != null) return _errorRetry(cs, _refresh);
    return const Center(child: CircularProgressIndicator());
  }
}

Widget _errorRetry(ColorScheme cs, Future<void> Function() onRetry) => Center(
  child: Column(
    mainAxisSize: MainAxisSize.min,
    children: [
      Icon(Icons.cloud_off, color: cs.onSurface.withValues(alpha: 0.4)),
      const SizedBox(height: 8),
      Text(
        'couldn’t load',
        style: TextStyle(color: cs.onSurface.withValues(alpha: 0.6)),
      ),
      const SizedBox(height: 8),
      OutlinedButton.icon(
        onPressed: onRetry,
        icon: const Icon(Icons.refresh),
        label: const Text('Retry'),
      ),
    ],
  ),
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
  // Runtime toggles (fire-and-forget; reflect the last value set this session).
  bool _planMode = false;
  String _brainMode = 'off';
  int _autoCont = 12;
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
      setState(
        () => _voiceError = _ttsKeySet
            ? 'Re-enter your API key to load voices'
            : 'Enter your API key first',
      );
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
      messenger.showSnackBar(
        const SnackBar(content: Text('voice settings saved')),
      );
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
          Builder(
            builder: (context) {
              final models = app.models;
              final current = app.session?.modelName ?? '';
              if (models.isEmpty) {
                return Text(
                  current.isEmpty ? '(loading…)' : current,
                  style: TextStyle(color: cs.onSurface.withValues(alpha: 0.7)),
                );
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
            },
          ),
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
                    leading: Icon(
                      app.activePersona == p.name
                          ? Icons.radio_button_checked
                          : Icons.radio_button_unchecked,
                    ),
                    title: Text(p.name),
                    subtitle: p.description.isEmpty
                        ? null
                        : Text(
                            p.description,
                            maxLines: 2,
                            overflow: TextOverflow.ellipsis,
                          ),
                    onTap: () =>
                        app.setPersona(p.name), // optimistic in AppController
                  ),
              ],
            ),
          const SizedBox(height: 8),
          SwitchListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text('YOLO — auto-approve tools'),
            subtitle: Text(
              'skip permission cards',
              style: TextStyle(color: cs.onSurface.withValues(alpha: 0.6)),
            ),
            value: app.yolo,
            onChanged: app.setYolo,
          ),
          const SizedBox(height: 8),
          _label('Runtime', cs),
          Row(
            children: [
              const Expanded(child: Text('Plan mode')),
              Switch(
                value: _planMode,
                onChanged: (v) {
                  setState(() => _planMode = v);
                  _api.setPlanMode(v);
                },
              ),
            ],
          ),
          const SizedBox(height: 4),
          Text(
            'Approval brain',
            style: TextStyle(
              fontSize: 12,
              color: cs.onSurface.withValues(alpha: 0.6),
            ),
          ),
          const SizedBox(height: 4),
          Wrap(
            spacing: 8,
            children: [
              for (final mode in const ['off', 'advisory', 'auto'])
                ChoiceChip(
                  label: Text(mode),
                  selected: _brainMode == mode,
                  onSelected: (_) {
                    setState(() => _brainMode = mode);
                    _api.setBrainMode(mode);
                  },
                ),
            ],
          ),
          const SizedBox(height: 8),
          Text(
            'Auto-continue steps',
            style: TextStyle(
              fontSize: 12,
              color: cs.onSurface.withValues(alpha: 0.6),
            ),
          ),
          const SizedBox(height: 4),
          Wrap(
            spacing: 8,
            children: [
              for (final n in const [0, 4, 12, 24])
                ChoiceChip(
                  label: Text('$n'),
                  selected: _autoCont == n,
                  onSelected: (_) {
                    setState(() => _autoCont = n);
                    _api.setAutoContinue(n);
                  },
                ),
            ],
          ),
          const Divider(height: 28),
          _label('Voice', cs),
          SwitchListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text('Enable voice'),
            value: _voiceEnabled,
            onChanged: (v) => setState(() => _voiceEnabled = v),
          ),
          Wrap(
            spacing: 8,
            children: [
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
            ],
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _ttsKey,
            obscureText: true,
            onSubmitted: _ttsProvider == 'elevenlabs'
                ? (_) => _loadVoices()
                : null,
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
            Row(
              children: [
                OutlinedButton.icon(
                  onPressed: _loadingVoices ? null : _loadVoices,
                  icon: _loadingVoices
                      ? const SizedBox(
                          width: 16,
                          height: 16,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(Icons.podcasts, size: 18),
                  label: Text(
                    _voices.isEmpty
                        ? 'Authenticate & load voices'
                        : 'Reload voices',
                  ),
                ),
                const SizedBox(width: 10),
                if (_voices.isNotEmpty)
                  Text(
                    '✓ ${_voices.length} voices',
                    style: const TextStyle(
                      color: Color(0xFF4FE0A0),
                      fontWeight: FontWeight.w600,
                    ),
                  ),
              ],
            ),
            if (_voiceError != null)
              Padding(
                padding: const EdgeInsets.only(top: 6),
                child: Text(
                  _voiceError!,
                  style: TextStyle(color: cs.error, fontSize: 12.5),
                ),
              ),
            const SizedBox(height: 12),
            if (_voices.isNotEmpty)
              DropdownButtonFormField<String>(
                initialValue: _voices.any((v) => v.id == _ttsVoice.text.trim())
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
                      child: Text(v.name, overflow: TextOverflow.ellipsis),
                    ),
                ],
                onChanged: (id) => setState(() => _ttsVoice.text = id ?? ''),
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
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
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
                label: Text(
                  'Restart gateway',
                  style: TextStyle(color: cs.error),
                ),
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
            ],
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
        SnackBar(
          content: Text('recover: ${r['action'] ?? r['error'] ?? 'ok'}'),
        ),
      );
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
          'the app reconnects automatically.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(c, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(c, true),
            child: const Text('Restart'),
          ),
        ],
      ),
    );
    if (ok != true) return;
    try {
      final r = await _api.selfRestart();
      m.showSnackBar(
        SnackBar(
          content: Text('restart: ${r['mode'] ?? (r['error'] ?? 'ok')}'),
        ),
      );
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
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              controller: keyC,
              decoration: const InputDecoration(
                labelText: 'dotted key, e.g. llm.temperature',
              ),
            ),
            TextField(
              controller: valC,
              decoration: const InputDecoration(
                labelText: 'value (JSON or text)',
              ),
            ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(c, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(c, true),
            child: const Text('Set + reload'),
          ),
        ],
      ),
    );
    if (ok != true || keyC.text.trim().isEmpty) return;
    try {
      final r = await _api.selfConfigSet(
        keyC.text.trim(),
        valC.text,
        reload: true,
      );
      m.showSnackBar(
        SnackBar(
          content: Text(
            r['ok'] == true
                ? (r['message']?.toString() ?? 'config set')
                : 'error: ${r['error']}',
          ),
        ),
      );
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
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              controller: nameC,
              decoration: const InputDecoration(labelText: 'name (slug)'),
            ),
            TextField(
              controller: descC,
              decoration: const InputDecoration(labelText: 'description'),
            ),
            TextField(
              controller: instrC,
              maxLines: 4,
              decoration: const InputDecoration(
                labelText: 'instructions (markdown)',
              ),
            ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(c, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(c, true),
            child: const Text('Save + reload'),
          ),
        ],
      ),
    );
    if (ok != true || nameC.text.trim().isEmpty) return;
    try {
      final r = await _api.skillWrite(
        nameC.text.trim(),
        descC.text.trim(),
        instrC.text,
        reload: true,
      );
      m.showSnackBar(
        SnackBar(
          content: Text(
            r['ok'] == true ? 'skill saved' : 'error: ${r['error']}',
          ),
        ),
      );
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
                _row(
                  'uptime',
                  _fmtUptime((st['uptime_secs'] as num?) ?? 0),
                  cs,
                ),
                _row('model', st['model']?.toString() ?? s.modelName, cs),
                _row('version', st['version']?.toString() ?? '—', cs),
                _row('accelerator', st['accel']?.toString() ?? 'cpu', cs),
                const SizedBox(height: 14),
                _label('context', cs),
                ClipRRect(
                  borderRadius: BorderRadius.circular(4),
                  child: LinearProgressIndicator(
                    value: s.contextFrac,
                    minHeight: 8,
                  ),
                ),
                const SizedBox(height: 2),
                Text(
                  '${(s.contextFrac * 100).round()}%',
                  style: const TextStyle(fontSize: 12),
                ),
                const SizedBox(height: 14),
                _row('tokens', '↑${s.inputTokens}  ↓${s.outputTokens}', cs),
                if (s.costUsd > 0)
                  _row('cost', '\$${s.costUsd.toStringAsFixed(4)}', cs),
                const SizedBox(height: 14),
                _label('working dir', cs),
                Text(
                  st['working_dir']?.toString() ?? '—',
                  style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
                ),
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
          child: Text(
            v,
            textAlign: TextAlign.right,
            style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
          ),
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
                color: running
                    ? cs.primary
                    : cs.onSurface.withValues(alpha: 0.6),
              ),
            ),
          ),
          const SizedBox(width: 8),
          running
              ? FilledButton.tonalIcon(
                  onPressed: _toggleLoop,
                  icon: const Icon(Icons.stop, size: 18),
                  label: const Text('Stop'),
                )
              : FilledButton.icon(
                  onPressed: _toggleLoop,
                  icon: const Icon(Icons.play_arrow, size: 18),
                  label: const Text('Run loop'),
                ),
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
            child: Text(
              '(no tasks — add with `blumi task add`)',
              style: TextStyle(color: cs.onSurface.withValues(alpha: 0.6)),
            ),
          );
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
                    child: Text(
                      '${state.toUpperCase()} · ${byState[state]!.length}',
                      style: TextStyle(
                        fontWeight: FontWeight.bold,
                        fontSize: 12,
                        color: cs.onSurface.withValues(alpha: 0.7),
                      ),
                    ),
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
            child: Text(
              'P${t.priority}',
              style: TextStyle(
                fontSize: 10,
                color: cs.onSurface.withValues(alpha: 0.7),
              ),
            ),
          ),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  t.title,
                  style: TextStyle(
                    decoration: t.state == 'done' || t.state == 'cancelled'
                        ? TextDecoration.lineThrough
                        : null,
                  ),
                ),
                if (t.detail.isNotEmpty)
                  Text(
                    t.detail,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 12,
                      color: cs.onSurface.withValues(alpha: 0.6),
                    ),
                  ),
                // Remote-runtime attribution: which grid peer is executing this.
                if (t.owner != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 2),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Icon(Icons.dns, size: 12, color: cs.tertiary),
                        const SizedBox(width: 3),
                        Text(
                          'remote · ${t.owner}',
                          style: TextStyle(
                            fontSize: 11,
                            color: cs.tertiary,
                            fontWeight: FontWeight.w500,
                          ),
                        ),
                      ],
                    ),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

// --- Grid (task delegation) ------------------------------------------------

/// Delegate a free-form task across the grid, deterministically over the API
/// (`POST /api/grid/delegate`) — no model tool-call needed, so it works on any
/// model. Pick a target (all peers, or one), type a task, see each machine's
/// result.
class _GridTab extends StatefulWidget {
  final AppController app;
  final ScrollController scroll;
  const _GridTab(this.app, this.scroll);
  @override
  State<_GridTab> createState() => _GridTabState();
}

class _GridTabState extends State<_GridTab> with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  ApiClient get _api => widget.app.session!.api;
  final _prompt = TextEditingController();
  String _target = 'all';
  List<GridPeer> _peers = [];
  bool _loadingPeers = true;
  bool _busy = false;
  String? _error;
  List<Map<String, dynamic>> _results = [];

  @override
  void initState() {
    super.initState();
    _loadPeers();
  }

  @override
  void dispose() {
    _prompt.dispose();
    super.dispose();
  }

  Future<void> _loadPeers() async {
    setState(() => _loadingPeers = true);
    try {
      final (peers, _) = await _api.gridPeers();
      if (!mounted) return;
      setState(() {
        _peers = peers;
        if (_target != 'all' && !peers.any((p) => p.name == _target)) {
          _target = 'all';
        }
        _loadingPeers = false;
      });
    } catch (_) {
      if (!mounted) return;
      setState(() {
        _peers = [];
        _loadingPeers = false;
      });
    }
  }

  Future<void> _delegate() async {
    final text = _prompt.text.trim();
    if (text.isEmpty || _busy) return;
    FocusScope.of(context).unfocus();
    setState(() {
      _busy = true;
      _error = null;
      _results = [];
    });
    try {
      final r = await _api.gridDelegate(text, target: _target);
      if (!mounted) return;
      if (r['ok'] == true) {
        setState(
          () => _results = ((r['results'] as List?) ?? [])
              .cast<Map<String, dynamic>>(),
        );
      } else {
        setState(() => _error = r['error']?.toString() ?? 'delegation failed');
      }
    } catch (e) {
      if (!mounted) return;
      setState(() => _error = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final cs = Theme.of(context).colorScheme;
    return ListView(
      controller: widget.scroll,
      padding: const EdgeInsets.all(16),
      children: [
        Row(
          children: [
            _label('Live peers', cs),
            const Spacer(),
            IconButton(
              tooltip: 'Refresh peers',
              visualDensity: VisualDensity.compact,
              icon: const Icon(Icons.refresh, size: 18),
              onPressed: _loadingPeers ? null : _loadPeers,
            ),
          ],
        ),
        if (_loadingPeers)
          const Padding(
            padding: EdgeInsets.all(8),
            child: LinearProgressIndicator(),
          )
        else if (_peers.isEmpty)
          Text(
            'no live grid peers',
            style: TextStyle(color: cs.onSurface.withValues(alpha: 0.6)),
          )
        else
          for (final p in _peers)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 3),
              child: Row(
                children: [
                  const Icon(Icons.dns, size: 16, color: Colors.greenAccent),
                  const SizedBox(width: 8),
                  Text(
                    p.name,
                    style: const TextStyle(fontWeight: FontWeight.w600),
                  ),
                  const SizedBox(width: 8),
                  Text(
                    p.host,
                    style: TextStyle(
                      fontSize: 12,
                      color: cs.onSurface.withValues(alpha: 0.6),
                    ),
                  ),
                ],
              ),
            ),
        const SizedBox(height: 18),
        _label('Delegate a task', cs),
        const SizedBox(height: 4),
        Row(
          children: [
            Text(
              'Run on ',
              style: TextStyle(color: cs.onSurface.withValues(alpha: 0.7)),
            ),
            DropdownButton<String>(
              value: _target,
              onChanged: _busy
                  ? null
                  : (v) => setState(() => _target = v ?? 'all'),
              items: [
                const DropdownMenuItem(
                  value: 'all',
                  child: Text('all peers (broadcast)'),
                ),
                for (final p in _peers)
                  DropdownMenuItem(value: p.name, child: Text(p.name)),
              ],
            ),
          ],
        ),
        const SizedBox(height: 8),
        TextField(
          controller: _prompt,
          minLines: 2,
          maxLines: 5,
          decoration: InputDecoration(
            hintText: 'e.g. Run `hostname` and report your OS and CPU count',
            border: const OutlineInputBorder(),
            isDense: true,
            filled: true,
            fillColor: cs.surfaceContainerHighest.withValues(alpha: 0.3),
          ),
        ),
        const SizedBox(height: 10),
        SizedBox(
          width: double.infinity,
          child: FilledButton.icon(
            onPressed: (_busy || _peers.isEmpty) ? null : _delegate,
            icon: _busy
                ? const SizedBox(
                    width: 16,
                    height: 16,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.hub, size: 18),
            label: Text(_busy ? 'Delegating…' : 'Delegate over grid'),
          ),
        ),
        if (_error != null) ...[
          const SizedBox(height: 10),
          Text(_error!, style: TextStyle(color: cs.error)),
        ],
        if (_results.isNotEmpty) ...[
          const SizedBox(height: 18),
          _label('Results (${_results.length})', cs),
          const SizedBox(height: 4),
          for (final r in _results) _resultCard(r, cs),
        ],
      ],
    );
  }

  Widget _resultCard(Map<String, dynamic> r, ColorScheme cs) {
    final ok = r['ok'] == true;
    final peer = r['peer']?.toString() ?? 'peer';
    final host = r['host']?.toString() ?? '';
    final ms = (r['ms'] as num?)?.toInt() ?? 0;
    final body = ok
        ? (r['output']?.toString() ?? '')
        : (r['error']?.toString() ?? 'failed');
    return Card(
      margin: const EdgeInsets.only(bottom: 8),
      child: Padding(
        padding: const EdgeInsets.all(10),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(
                  ok ? Icons.check_circle : Icons.error,
                  size: 16,
                  color: ok ? Colors.greenAccent : cs.error,
                ),
                const SizedBox(width: 6),
                Text(peer, style: const TextStyle(fontWeight: FontWeight.bold)),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                    host,
                    style: TextStyle(
                      fontSize: 11,
                      color: cs.onSurface.withValues(alpha: 0.55),
                    ),
                  ),
                ),
                Text(
                  '${(ms / 1000).toStringAsFixed(1)}s',
                  style: TextStyle(
                    fontSize: 11,
                    color: cs.onSurface.withValues(alpha: 0.55),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 6),
            SelectableText(
              body,
              style: TextStyle(
                fontFamily: 'monospace',
                fontSize: 12.5,
                color: ok ? cs.onSurface : cs.error,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

// --- Code knowledge base ---------------------------------------------------

class _KnowledgeTab extends StatefulWidget {
  final AppController app;
  final ScrollController scroll;
  const _KnowledgeTab(this.app, this.scroll);
  @override
  State<_KnowledgeTab> createState() => _KnowledgeTabState();
}

class _KnowledgeTabState extends State<_KnowledgeTab>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  ApiClient get _api => widget.app.session!.api;
  final _path = TextEditingController();
  final _query = TextEditingController();
  Map<String, dynamic> _status = {};
  List<Map<String, dynamic>> _sources = [];
  List<Map<String, dynamic>> _hits = [];
  bool _busy = false;
  bool _searching = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  @override
  void dispose() {
    _path.dispose();
    _query.dispose();
    super.dispose();
  }

  Future<void> _refresh() async {
    try {
      final st = await _api.knowledgeStatus();
      final src = await _api.knowledgeSources();
      if (!mounted) return;
      setState(() {
        _status = st;
        _sources = ((src['sources'] as List?) ?? [])
            .cast<Map<String, dynamic>>();
      });
    } catch (_) {}
  }

  Future<void> _ingest() async {
    final p = _path.text.trim();
    if (p.isEmpty || _busy) return;
    FocusScope.of(context).unfocus();
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final r = await _api.knowledgeIngest(p);
      if (r['ok'] != true) {
        setState(() => _error = r['error']?.toString() ?? 'ingest failed');
        return;
      }
      await _pollIngest();
    } catch (e) {
      if (mounted) setState(() => _error = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  /// Poll status until the background ingest finishes (bounded).
  Future<void> _pollIngest() async {
    for (var i = 0; i < 600; i++) {
      await Future.delayed(const Duration(seconds: 2));
      if (!mounted) return;
      try {
        final st = await _api.knowledgeStatus();
        if (!mounted) return;
        setState(() => _status = st);
        if (st['ingesting'] != true) {
          await _refresh();
          return;
        }
      } catch (_) {
        return;
      }
    }
  }

  Future<void> _search() async {
    final q = _query.text.trim();
    if (q.isEmpty || _searching) return;
    FocusScope.of(context).unfocus();
    setState(() {
      _searching = true;
      _error = null;
      _hits = [];
    });
    try {
      final r = await _api.knowledgeSearch(q, limit: 12);
      if (!mounted) return;
      setState(
        () => _hits = ((r['hits'] as List?) ?? []).cast<Map<String, dynamic>>(),
      );
    } catch (e) {
      if (mounted) setState(() => _error = '$e');
    } finally {
      if (mounted) setState(() => _searching = false);
    }
  }

  Future<void> _remove(String source) async {
    try {
      await _api.knowledgeRemove(source);
      await _refresh();
    } catch (_) {}
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final cs = Theme.of(context).colorScheme;
    final enabled = _status['enabled'] == true;
    final ingesting = _status['ingesting'] == true;
    return ListView(
      controller: widget.scroll,
      padding: const EdgeInsets.all(16),
      children: [
        if (!enabled)
          Text(
            'Code knowledge base is disabled (set knowledge.enabled).',
            style: TextStyle(color: cs.onSurface.withValues(alpha: 0.6)),
          )
        else ...[
          Row(
            children: [
              _label('Knowledge base', cs),
              const Spacer(),
              IconButton(
                tooltip: 'Refresh',
                visualDensity: VisualDensity.compact,
                icon: const Icon(Icons.refresh, size: 18),
                onPressed: _refresh,
              ),
            ],
          ),
          Text(
            '${_status['files'] ?? 0} files · ${_status['symbols'] ?? 0} symbols · ${_status['vectors'] ?? 0} vectors',
            style: TextStyle(color: cs.onSurface.withValues(alpha: 0.7)),
          ),
          const SizedBox(height: 14),
          _label('Index a repo (path on the gateway machine)', cs),
          const SizedBox(height: 4),
          TextField(
            controller: _path,
            decoration: InputDecoration(
              hintText: '/Users/you/code/my-repo',
              border: const OutlineInputBorder(),
              isDense: true,
              filled: true,
              fillColor: cs.surfaceContainerHighest.withValues(alpha: 0.3),
            ),
          ),
          const SizedBox(height: 8),
          SizedBox(
            width: double.infinity,
            child: FilledButton.icon(
              onPressed: (_busy || ingesting) ? null : _ingest,
              icon: (_busy || ingesting)
                  ? const SizedBox(
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Icon(Icons.account_tree, size: 18),
              label: Text((_busy || ingesting) ? 'Indexing…' : 'Index'),
            ),
          ),
          if ((_status['message']?.toString() ?? '').isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: 6),
              child: Text(
                _status['message'].toString(),
                style: TextStyle(
                  fontSize: 12,
                  color: cs.onSurface.withValues(alpha: 0.6),
                ),
              ),
            ),
          if (_sources.isNotEmpty) ...[
            const SizedBox(height: 12),
            _label('Sources (${_sources.length})', cs),
            for (final s in _sources)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 2),
                child: Row(
                  children: [
                    Expanded(
                      child: Text(
                        '${s['source']}',
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(fontSize: 12.5),
                      ),
                    ),
                    Text(
                      '${s['symbols']}',
                      style: TextStyle(
                        fontSize: 11,
                        color: cs.onSurface.withValues(alpha: 0.55),
                      ),
                    ),
                    IconButton(
                      tooltip: 'Remove',
                      visualDensity: VisualDensity.compact,
                      icon: const Icon(Icons.delete_outline, size: 16),
                      onPressed: () => _remove('${s['source']}'),
                    ),
                  ],
                ),
              ),
          ],
          const SizedBox(height: 18),
          _label('Search code', cs),
          const SizedBox(height: 4),
          TextField(
            controller: _query,
            onSubmitted: (_) => _search(),
            decoration: InputDecoration(
              hintText: 'e.g. where is the permission engine created',
              border: const OutlineInputBorder(),
              isDense: true,
              filled: true,
              fillColor: cs.surfaceContainerHighest.withValues(alpha: 0.3),
              suffixIcon: IconButton(
                icon: _searching
                    ? const SizedBox(
                        width: 16,
                        height: 16,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.search),
                onPressed: _searching ? null : _search,
              ),
            ),
          ),
          if (_error != null) ...[
            const SizedBox(height: 10),
            Text(_error!, style: TextStyle(color: cs.error)),
          ],
          if (_hits.isNotEmpty) ...[
            const SizedBox(height: 14),
            _label('Results (${_hits.length})', cs),
            for (final h in _hits) _hitCard(h, cs),
          ],
        ],
      ],
    );
  }

  Widget _hitCard(Map<String, dynamic> h, ColorScheme cs) {
    final path = '${h['path'] ?? ''}';
    final parts = path.split('/');
    final short = parts.length > 2
        ? '…/${parts.sublist(parts.length - 2).join('/')}'
        : path;
    return Card(
      margin: const EdgeInsets.only(bottom: 8),
      child: Padding(
        padding: const EdgeInsets.all(10),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(Icons.code, size: 15, color: cs.primary),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                    '${h['name'] ?? ''}',
                    style: const TextStyle(fontWeight: FontWeight.bold),
                  ),
                ),
                Text(
                  '${h['kind'] ?? ''}',
                  style: TextStyle(
                    fontSize: 11,
                    color: cs.onSurface.withValues(alpha: 0.55),
                  ),
                ),
                IconButton(
                  tooltip: 'Impact (change blast radius)',
                  visualDensity: VisualDensity.compact,
                  padding: EdgeInsets.zero,
                  constraints: const BoxConstraints(),
                  icon: const Icon(Icons.account_tree_outlined, size: 16),
                  onPressed: () => _showImpact('${h['name'] ?? ''}'),
                ),
              ],
            ),
            Text(
              '$short:${h['start_line'] ?? 0}',
              style: TextStyle(
                fontSize: 11,
                color: cs.onSurface.withValues(alpha: 0.55),
              ),
            ),
            const SizedBox(height: 6),
            SelectableText(
              '${h['snippet'] ?? ''}',
              maxLines: 8,
              style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
            ),
          ],
        ),
      ),
    );
  }

  /// Bottom sheet: the transitive callers of `symbol` from the code graph — the
  /// change blast radius (what could break if you edit it).
  Future<void> _showImpact(String symbol) async {
    if (symbol.isEmpty) return;
    List<Map<String, dynamic>> rel = [];
    String? err;
    try {
      final r = await _api.knowledgeGraph('impact', symbol, limit: 40);
      rel = ((r['hits'] as List?) ?? []).cast<Map<String, dynamic>>();
    } catch (e) {
      err = '$e';
    }
    if (!mounted) return;
    final cs = Theme.of(context).colorScheme;
    showModalBottomSheet(
      context: context,
      showDragHandle: true,
      builder: (_) => SafeArea(
        child: Padding(
          padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                'Impact of $symbol',
                style: const TextStyle(fontWeight: FontWeight.bold, fontSize: 15),
              ),
              Text(
                'Symbols that (transitively) depend on it — the change blast radius.',
                style: TextStyle(
                  fontSize: 12,
                  color: cs.onSurface.withValues(alpha: 0.6),
                ),
              ),
              const SizedBox(height: 12),
              if (err != null)
                Text(err, style: TextStyle(color: cs.error))
              else if (rel.isEmpty)
                Text(
                  'No dependents found. Index with the structural graph '
                  '(knowledge.graph.mode=structural) for precise edges.',
                  style: TextStyle(color: cs.onSurface.withValues(alpha: 0.6)),
                )
              else
                Flexible(
                  child: ListView(
                    shrinkWrap: true,
                    children: [
                      for (final h in rel)
                        ListTile(
                          dense: true,
                          leading: Icon(Icons.code, size: 16, color: cs.primary),
                          title: Text('${h['name'] ?? ''}'),
                          subtitle: Text(
                            '${h['path'] ?? ''}:${h['start_line'] ?? 0}',
                            style: const TextStyle(fontSize: 11),
                          ),
                        ),
                    ],
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

// --- Plans -----------------------------------------------------------------

class _PlansTab extends StatefulWidget {
  final AppController app;
  final ScrollController scroll;
  const _PlansTab(this.app, this.scroll);
  @override
  State<_PlansTab> createState() => _PlansTabState();
}

class _PlansTabState extends State<_PlansTab>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  ApiClient get _api => widget.app.session!.api;
  List<Map<String, dynamic>> _plans = [];
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final p = await _api.plans();
      if (!mounted) return;
      setState(() {
        _plans = p.reversed.toList(); // newest first
        _loading = false;
      });
    } catch (_) {
      if (!mounted) return;
      setState(() => _loading = false);
    }
  }

  (Color, String) _badge(String status, ColorScheme cs) {
    switch (status) {
      case 'live':
        return (Colors.greenAccent, 'live');
      case 'rejected':
        return (cs.error, 'rejected');
      default:
        return (Colors.green, 'approved');
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final cs = Theme.of(context).colorScheme;
    return ListView(
      controller: widget.scroll,
      padding: const EdgeInsets.all(16),
      children: [
        Row(children: [
          _label('Proposed plans', cs),
          const Spacer(),
          IconButton(
            tooltip: 'Refresh',
            visualDensity: VisualDensity.compact,
            icon: const Icon(Icons.refresh, size: 18),
            onPressed: _loading ? null : _load,
          ),
        ]),
        if (_loading)
          const Padding(
              padding: EdgeInsets.all(8), child: LinearProgressIndicator())
        else if (_plans.isEmpty)
          Text(
            'No plans yet. Approve or reject a plan in plan mode and it shows up here.',
            style: TextStyle(color: cs.onSurface.withValues(alpha: 0.6)),
          )
        else
          for (final p in _plans) _planCard(p, cs),
      ],
    );
  }

  Widget _planCard(Map<String, dynamic> p, ColorScheme cs) {
    final status = '${p['status'] ?? 'approved'}';
    final (dotColor, label) = _badge(status, cs);
    final title = '${p['title'] ?? '(untitled plan)'}';
    final content = '${p['content'] ?? ''}';
    return Card(
      margin: const EdgeInsets.only(bottom: 8),
      child: Theme(
        data: Theme.of(context).copyWith(dividerColor: Colors.transparent),
        child: ExpansionTile(
          tilePadding: const EdgeInsets.symmetric(horizontal: 12),
          leading: Icon(Icons.circle, size: 12, color: dotColor),
          title: Text(
            title,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: const TextStyle(fontWeight: FontWeight.w600),
          ),
          subtitle: Text(label, style: TextStyle(fontSize: 11, color: dotColor)),
          childrenPadding: const EdgeInsets.fromLTRB(12, 0, 12, 12),
          children: [
            SelectableText(
              content,
              style: const TextStyle(fontFamily: 'monospace', fontSize: 12.5),
            ),
          ],
        ),
      ),
    );
  }
}

// --- Memory graph (D3-style) -----------------------------------------------

class _GraphTab extends StatefulWidget {
  final AppController app;
  final ScrollController scroll;
  const _GraphTab(this.app, this.scroll);
  @override
  State<_GraphTab> createState() => _GraphTabState();
}

class _GraphTabState extends State<_GraphTab>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  ApiClient get _api => widget.app.session!.api;
  late final WebViewController _wv;
  final _q = TextEditingController();
  bool _ready = false;
  bool _busy = false;
  String? _pendingJson;

  @override
  void initState() {
    super.initState();
    _wv = WebViewController()
      ..setJavaScriptMode(JavaScriptMode.unrestricted)
      ..setBackgroundColor(const Color(0xFF16090E))
      ..setNavigationDelegate(NavigationDelegate(onPageFinished: (_) {
        _ready = true;
        if (_pendingJson != null) {
          _wv.runJavaScript('render($_pendingJson)');
          _pendingJson = null;
        }
      }))
      ..loadFlutterAsset('assets/memory_graph.html');
  }

  @override
  void dispose() {
    _q.dispose();
    super.dispose();
  }

  Future<void> _search() async {
    final q = _q.text.trim();
    if (q.isEmpty || _busy) return;
    FocusScope.of(context).unfocus();
    setState(() => _busy = true);
    try {
      final data = await _api.memoryGraph(q, limit: 50);
      final js = jsonEncode(data);
      if (_ready) {
        await _wv.runJavaScript('render($js)');
      } else {
        _pendingJson = js;
      }
    } catch (_) {
      // leave the canvas as-is
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final cs = Theme.of(context).colorScheme;
    return Column(children: [
      Padding(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 6),
        child: TextField(
          controller: _q,
          textInputAction: TextInputAction.search,
          onSubmitted: (_) => _search(),
          decoration: InputDecoration(
            hintText: 'Search memory → graph',
            isDense: true,
            border: const OutlineInputBorder(),
            filled: true,
            fillColor: cs.surfaceContainerHighest.withValues(alpha: 0.3),
            suffixIcon: IconButton(
              icon: _busy
                  ? const SizedBox(
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(strokeWidth: 2))
                  : const Icon(Icons.search),
              onPressed: _busy ? null : _search,
            ),
          ),
        ),
      ),
      Expanded(
        child: ClipRRect(
          borderRadius: BorderRadius.circular(10),
          child: WebViewWidget(controller: _wv),
        ),
      ),
    ]);
  }
}

// --- Self-healing ----------------------------------------------------------

class _HealTab extends StatelessWidget {
  final AppController app;
  final ScrollController scroll;
  const _HealTab(this.app, this.scroll);

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return _AsyncView<Map<String, dynamic>>(
      cache: app.cache,
      cacheKey: app.ck('heal'),
      ttl: const Duration(seconds: 15),
      fetch: () => app.session!.api.healStatus(),
      parse: (raw) => (raw as Map).cast<String, dynamic>(),
      builder: (context, data, refresh) {
        final counts = (data['counts'] as Map?)?.cast<String, dynamic>() ?? {};
        final recent = ((data['recent'] as List?) ?? []).cast<Map>();
        int c(String k) => (counts[k] as num?)?.toInt() ?? 0;
        if (recent.isEmpty && c('recovery') == 0 && c('evolution') == 0) {
          return Center(
            child: Text(
              'No self-healing activity yet.\nRecoveries + learned fixes show up here.',
              textAlign: TextAlign.center,
              style: TextStyle(color: cs.onSurface.withValues(alpha: 0.6)),
            ),
          );
        }
        IconData icon(String kind) {
          switch (kind) {
            case 'evolution':
              return Icons.auto_awesome;
            case 'evolution_proposal':
              return Icons.lightbulb_outline;
            default:
              return Icons.healing;
          }
        }

        return ListView(
          controller: scroll,
          padding: const EdgeInsets.all(16),
          children: [
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                _healChip('recoveries', c('recovery'), cs),
                _healChip('evolved', c('evolution'), cs),
                _healChip('proposed', c('evolution_proposal'), cs),
              ],
            ),
            const SizedBox(height: 14),
            Text(
              'Recent',
              style: TextStyle(fontWeight: FontWeight.w600, color: cs.primary),
            ),
            const SizedBox(height: 6),
            for (final r in recent)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 4),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Icon(
                      icon(r['kind']?.toString() ?? ''),
                      size: 16,
                      color: cs.primary,
                    ),
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(
                        r['text']?.toString() ?? '',
                        style: const TextStyle(
                          fontSize: 12,
                          fontFamily: 'monospace',
                        ),
                      ),
                    ),
                  ],
                ),
              ),
          ],
        );
      },
    );
  }

  Widget _healChip(String label, int n, ColorScheme cs) => Container(
    padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
    decoration: BoxDecoration(
      color: cs.primary.withValues(alpha: 0.12),
      borderRadius: BorderRadius.circular(8),
    ),
    child: Text(
      '$n $label',
      style: TextStyle(
        color: cs.primary,
        fontWeight: FontWeight.w600,
        fontSize: 12,
      ),
    ),
  );
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
            child: Text(
              '(no usage yet)',
              style: TextStyle(color: cs.onSurface.withValues(alpha: 0.6)),
            ),
          );
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
                    Text(
                      '${e.value}',
                      style: const TextStyle(
                        fontSize: 13,
                        fontFamily: 'monospace',
                      ),
                    ),
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
            child: Text(
              '(no skills)',
              style: TextStyle(color: cs.onSurface.withValues(alpha: 0.6)),
            ),
          );
        }
        return ListView(
          controller: scroll,
          padding: const EdgeInsets.all(16),
          children: [
            for (final s in skills)
              ListTile(
                dense: true,
                contentPadding: EdgeInsets.zero,
                leading: Icon(
                  Icons.auto_awesome,
                  size: 18,
                  color: cs.secondary,
                ),
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
    child: SelectableText(
      text,
      style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
    ),
  );
}

/// A small uppercase section label — the control center's shared header style
/// (matches the kit's SectionHeader; takes a ColorScheme so it works in the
/// many places here that only have `cs`).
Widget _label(String t, ColorScheme cs) => Padding(
  padding: const EdgeInsets.only(top: 12, bottom: 6),
  child: Text(
    t.toUpperCase(),
    style: TextStyle(
      fontWeight: FontWeight.w700,
      color: cs.onSurface.withValues(alpha: 0.55),
      fontSize: 11,
      letterSpacing: 1.1,
    ),
  ),
);

/// Retrospection tab: this node's run-log + recent learnings + Run now / Rebuild,
/// plus a per-node summary across the grid.
class _RetroTab extends StatefulWidget {
  final AppController app;
  final ScrollController scroll;
  const _RetroTab(this.app, this.scroll);
  @override
  State<_RetroTab> createState() => _RetroTabState();
}

class _RetroTabState extends State<_RetroTab>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;

  ApiClient get _api => widget.app.session!.api;
  Map<String, dynamic> _status = {};
  List<Map<String, dynamic>> _nodes = [];
  bool _loading = true;
  bool _busy = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final st = await _api.retrospectStatus();
      final nodes = <Map<String, dynamic>>[];
      try {
        final g = await _api.gridMetrics();
        final self = (g['self'] as Map?)?.cast<String, dynamic>();
        if (self != null && self['retrospect'] != null) {
          nodes.add({
            'name': '${st['node'] ?? 'this node'}',
            'retrospect': self['retrospect'],
            'online': true,
            'self': true,
          });
        }
        for (final p in (g['peers'] as List?) ?? const []) {
          final pm = (p as Map).cast<String, dynamic>();
          final m = (pm['metrics'] as Map?)?.cast<String, dynamic>();
          nodes.add({
            'name': pm['name'] ?? '?',
            'retrospect': m?['retrospect'],
            'online': pm['online'] == true,
            'self': false,
          });
        }
      } catch (_) {}
      if (!mounted) return;
      setState(() {
        _status = st;
        _nodes = nodes;
        _error = null;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  Future<void> _run({required bool rebuild}) async {
    if (rebuild) {
      final ok = await showDialog<bool>(
        context: context,
        builder: (_) => AlertDialog(
          title: const Text('Rebuild memory from all chat?'),
          content: const Text(
            'Resets the watermark and replays the full history into memory. '
            'This can make many LLM calls; duplicates are merged.',
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: const Text('Rebuild'),
            ),
          ],
        ),
      );
      if (ok != true) return;
    }
    setState(() => _busy = true);
    try {
      final r = await _api.retrospectRun(rebuild: rebuild);
      if (!mounted) return;
      final ok = r['ok'] == true;
      final msg = ok
          ? 'Consolidated ${r['stored'] ?? 0} learning(s) from ${r['sessions'] ?? 0} session(s)'
          : 'Failed: ${r['error'] ?? 'unknown'}';
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
      await _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('$e')));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final cs = Theme.of(context).colorScheme;
    if (_loading) return const Center(child: CircularProgressIndicator());
    final enabled = _status['enabled'] == true;
    final runs = (((_status['runs'] as List?) ?? const []).reversed).toList();
    final learnings = (_status['learnings'] as List?) ?? const [];
    return ListView(
      controller: widget.scroll,
      padding: const EdgeInsets.all(16),
      children: [
        Row(children: [
          _label('Retrospection', cs),
          const Spacer(),
          IconButton(
            tooltip: 'Refresh',
            visualDensity: VisualDensity.compact,
            icon: const Icon(Icons.refresh, size: 18),
            onPressed: _busy ? null : _load,
          ),
        ]),
        Text(
          enabled
              ? 'Daily memory consolidation · every ${_status['hours'] ?? 24}h · last ${_fmt(_status['last_run'])}'
              : 'Disabled — set memory.retrospect',
          style: TextStyle(
              color: cs.onSurface.withValues(alpha: 0.7), fontSize: 12.5),
        ),
        if (_error != null)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(_error!, style: TextStyle(color: cs.error)),
          ),
        const SizedBox(height: 12),
        Row(children: [
          Expanded(
            child: OutlinedButton.icon(
              onPressed: _busy ? null : () => _run(rebuild: false),
              icon: _busy
                  ? const SizedBox(
                      width: 14,
                      height: 14,
                      child: CircularProgressIndicator(strokeWidth: 2))
                  : const Icon(Icons.play_arrow, size: 18),
              label: const Text('Run now'),
            ),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: OutlinedButton.icon(
              onPressed: _busy ? null : () => _run(rebuild: true),
              icon: const Icon(Icons.restart_alt, size: 18),
              label: const Text('Rebuild'),
            ),
          ),
        ]),
        const SizedBox(height: 18),
        _label('Run log (${runs.length})', cs),
        if (runs.isEmpty)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 6),
            child: Text('No runs yet.',
                style: TextStyle(color: cs.onSurface.withValues(alpha: 0.55))),
          )
        else
          for (final r in runs) _runRow((r as Map).cast<String, dynamic>(), cs),
        const SizedBox(height: 18),
        _label('Recent learnings (${learnings.length})', cs),
        if (learnings.isEmpty)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 6),
            child: Text('Nothing consolidated yet.',
                style: TextStyle(color: cs.onSurface.withValues(alpha: 0.55))),
          )
        else
          for (final l in learnings)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 3),
              child:
                  Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
                Icon(Icons.psychology_outlined, size: 15, color: cs.primary),
                const SizedBox(width: 6),
                Expanded(
                    child:
                        Text('$l', style: const TextStyle(fontSize: 12.5))),
              ]),
            ),
        const SizedBox(height: 18),
        _label('Across the grid (${_nodes.length})', cs),
        if (_nodes.isEmpty)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 6),
            child: Text('No grid peers (or grid disabled).',
                style: TextStyle(color: cs.onSurface.withValues(alpha: 0.55))),
          )
        else
          for (final n in _nodes) _nodeCard(n, cs),
      ],
    );
  }

  Widget _runRow(Map<String, dynamic> r, ColorScheme cs) => Padding(
        padding: const EdgeInsets.symmetric(vertical: 3),
        child: Row(children: [
          _kindChip('${r['kind'] ?? 'auto'}', cs),
          const SizedBox(width: 8),
          Expanded(
              child: Text(_fmt(r['at']), style: const TextStyle(fontSize: 12))),
          Text('+${r['stored'] ?? 0} · ${r['sessions'] ?? 0} sess',
              style: TextStyle(
                  fontSize: 11, color: cs.onSurface.withValues(alpha: 0.6))),
        ]),
      );

  Widget _nodeCard(Map<String, dynamic> n, ColorScheme cs) {
    final retro = (n['retrospect'] as Map?)?.cast<String, dynamic>();
    final last = (retro?['last'] as Map?)?.cast<String, dynamic>();
    final online = n['online'] == true;
    final sub = retro == null
        ? (online ? 'no data' : 'offline')
        : 'last ${_fmt(retro['last_run'])} · ${retro['runs'] ?? 0} runs${last != null ? ' · +${last['stored'] ?? 0}' : ''}';
    return Card(
      margin: const EdgeInsets.only(bottom: 8),
      child: Padding(
        padding: const EdgeInsets.all(10),
        child: Row(children: [
          Icon(n['self'] == true ? Icons.smartphone : Icons.hub_outlined,
              size: 16, color: cs.primary),
          const SizedBox(width: 8),
          Expanded(
            child:
                Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text('${n['name']}',
                  style: const TextStyle(
                      fontWeight: FontWeight.bold, fontSize: 13)),
              Text(sub,
                  style: TextStyle(
                      fontSize: 11,
                      color: cs.onSurface.withValues(alpha: 0.6))),
            ]),
          ),
          if (retro != null && retro['enabled'] == false)
            Text('off',
                style: TextStyle(
                    fontSize: 10, color: cs.onSurface.withValues(alpha: 0.5))),
        ]),
      ),
    );
  }

  Widget _kindChip(String kind, ColorScheme cs) {
    final c = kind == 'rebuild'
        ? cs.tertiary
        : kind == 'manual'
            ? cs.primary
            : cs.onSurface.withValues(alpha: 0.5);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
      decoration: BoxDecoration(
          color: c.withValues(alpha: 0.15),
          borderRadius: BorderRadius.circular(4)),
      child: Text(kind,
          style:
              TextStyle(fontSize: 10, color: c, fontWeight: FontWeight.w600)),
    );
  }

  String _fmt(dynamic ts) {
    if (ts == null) return 'never';
    final dt = DateTime.tryParse('$ts');
    if (dt == null) return '$ts';
    final diff = DateTime.now().difference(dt.toLocal());
    if (diff.inMinutes < 1) return 'just now';
    if (diff.inMinutes < 60) return '${diff.inMinutes}m ago';
    if (diff.inHours < 24) return '${diff.inHours}h ago';
    return '${diff.inDays}d ago';
  }
}
