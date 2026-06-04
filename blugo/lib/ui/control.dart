import 'dart:async';
import 'package:flutter/material.dart';
import '../data/api.dart';
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

/// FutureBuilder that shows a spinner while loading, a Retry on error (instead
/// of spinning forever), and [builder] on success. [load] (re)creates the
/// future; the builder gets a `refresh` callback.
class _AsyncView<T> extends StatefulWidget {
  final Future<T> Function() load;
  final Widget Function(BuildContext, T, Future<void> Function()) builder;
  const _AsyncView({required this.load, required this.builder});
  @override
  State<_AsyncView<T>> createState() => _AsyncViewState<T>();
}

class _AsyncViewState<T> extends State<_AsyncView<T>> {
  late Future<T> _f;
  @override
  void initState() {
    super.initState();
    _f = widget.load();
  }

  Future<void> _refresh() {
    final f = widget.load();
    setState(() => _f = f);
    return f.then((_) {}).catchError((_) {});
  }

  @override
  Widget build(BuildContext context) {
    return FutureBuilder<T>(
      future: _f,
      builder: (context, snap) {
        if (snap.connectionState == ConnectionState.waiting) {
          return const Center(child: CircularProgressIndicator());
        }
        if (snap.hasError || !snap.hasData) {
          return _errorRetry(Theme.of(context).colorScheme, _refresh);
        }
        return widget.builder(context, snap.data as T, _refresh);
      },
    );
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
  late Future<List<String>> _models;
  late Future<(List<PersonaInfo>, String)> _personas;

  // Voice config (loaded from /api/settings; key fields are write-only).
  final _ttsKey = TextEditingController();
  final _ttsVoice = TextEditingController();
  final _sttKey = TextEditingController();
  String _ttsProvider = 'elevenlabs';
  bool _voiceEnabled = false;
  bool _ttsKeySet = false;

  ApiClient get _api => widget.app.session!.api;

  @override
  void initState() {
    super.initState();
    _models = _api.models();
    _personas = _api.personas();
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
          FutureBuilder<List<String>>(
            future: _models,
            builder: (context, snap) {
              final models = snap.data ?? [];
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
                        app.session?.applyModel(m); // optimistic: UI updates now
                        _api.setModel(m); // fire-and-forget
                      },
                    ),
                ],
              );
            },
          ),
          const SizedBox(height: 18),
          _label('Persona', cs),
          FutureBuilder<(List<PersonaInfo>, String)>(
            future: _personas,
            builder: (context, snap) {
              final data = snap.data;
              if (data == null) return const Text('(loading…)');
              final (list, active) = data;
              return Column(
                children: [
                  for (final p in list)
                    ListTile(
                      dense: true,
                      contentPadding: EdgeInsets.zero,
                      selected: active == p.name,
                      leading: Icon(active == p.name
                          ? Icons.radio_button_checked
                          : Icons.radio_button_unchecked),
                      title: Text(p.name),
                      subtitle: p.description.isEmpty
                          ? null
                          : Text(p.description,
                              maxLines: 2, overflow: TextOverflow.ellipsis),
                      onTap: () {
                        // optimistic: move the selection now, then persist
                        setState(() => _personas = Future.value((list, p.name)));
                        _api.setPersona(p.name);
                      },
                    ),
                ],
              );
            },
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
                onSelected: (_) => setState(() => _ttsProvider = p),
              ),
          ]),
          const SizedBox(height: 12),
          TextField(
            controller: _ttsKey,
            obscureText: true,
            decoration: InputDecoration(
              labelText: _ttsProvider == 'elevenlabs'
                  ? 'ElevenLabs API key'
                  : 'TTS API key',
              hintText: _ttsKeySet ? 'saved ✓ — blank keeps current' : null,
              border: const OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _ttsVoice,
            decoration: InputDecoration(
              labelText:
                  _ttsProvider == 'elevenlabs' ? 'Voice ID' : 'Voice (e.g. alloy)',
              border: const OutlineInputBorder(),
            ),
          ),
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
          const SizedBox(height: 8),
        ],
      ),
    );
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
    return FutureBuilder<Map<String, dynamic>>(
      future: app.session!.api.usage(),
      builder: (context, snap) {
        if (!snap.hasData) {
          return const Center(child: CircularProgressIndicator());
        }
        final u = snap.data!;
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
    return FutureBuilder<List<String>>(
      future: app.session!.api.skills(),
      builder: (context, snap) {
        if (!snap.hasData) {
          return const Center(child: CircularProgressIndicator());
        }
        final skills = snap.data!;
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
    return FutureBuilder<(String, String)>(
      future: app.session!.api.memory(),
      builder: (context, snap) {
        if (!snap.hasData) {
          return const Center(child: CircularProgressIndicator());
        }
        final (project, user) = snap.data!;
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
