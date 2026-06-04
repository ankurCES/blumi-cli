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
      length: 4,
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
              Tab(text: 'Usage'),
              Tab(text: 'Skills'),
              Tab(text: 'Memory'),
            ],
          ),
          Expanded(
            child: TabBarView(children: [
              _SettingsTab(app, scroll),
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

  ApiClient get _api => widget.app.session!.api;

  @override
  void initState() {
    super.initState();
    _models = _api.models();
    _personas = _api.personas();
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
                      onSelected: (_) async {
                        await _api.setModel(m);
                        app.session?.applyModel(m);
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
                      onTap: () async {
                        await _api.setPersona(p.name);
                        if (mounted) {
                          setState(
                              () => _personas = Future.value((list, p.name)));
                        }
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
