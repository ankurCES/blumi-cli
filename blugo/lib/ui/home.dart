import 'package:flutter/material.dart';
import '../data/events.dart';
import '../data/models.dart';
import '../data/voice.dart';
import '../state/app.dart';
import '../state/session.dart';
import 'control.dart';
import 'markdown.dart';
import 'palette.dart';
import 'thinking.dart';

/// Fold-responsive shell. Wide (fold-open) shows explorer | chat | agent rail;
/// narrow (portrait) shows chat with the explorer + agent rail as drawers —
/// mirroring the TUI's 3-pane workbench. Driven off window width, not
/// orientation, so unfolding re-lays out live.
class HomeShell extends StatelessWidget {
  final AppController app;
  const HomeShell(this.app, {super.key});

  static const double _wide = 840;

  VoidCallback _cmd(BuildContext context) =>
      () => showCommandPalette(context, app);

  @override
  Widget build(BuildContext context) {
    final session = app.session!;
    return LayoutBuilder(
      builder: (context, c) {
        final wide = c.maxWidth >= _wide;
        return Scaffold(
          appBar: _Header(app, showMenus: !wide),
          drawer: wide ? null : Drawer(child: SafeArea(child: SessionsPane(app))),
          endDrawer:
              wide ? null : Drawer(child: SafeArea(child: AgentRail(session))),
          body: SafeArea(
            child: wide
                ? Row(
                    children: [
                      SizedBox(width: 260, child: SessionsPane(app)),
                      const VerticalDivider(width: 1),
                      Expanded(child: ChatPane(session, onCommand: _cmd(context))),
                      const VerticalDivider(width: 1),
                      SizedBox(width: 320, child: AgentRail(session)),
                    ],
                  )
                : ChatPane(session, onCommand: _cmd(context)),
          ),
        );
      },
    );
  }
}

class _Header extends StatelessWidget implements PreferredSizeWidget {
  final AppController app;
  final bool showMenus;
  const _Header(this.app, {required this.showMenus});
  @override
  Size get preferredSize => const Size.fromHeight(kToolbarHeight);

  @override
  Widget build(BuildContext context) {
    final s = app.session!;
    final cs = Theme.of(context).colorScheme;
    return AppBar(
      title: Row(children: [
        Text('✿ blumi', style: TextStyle(color: cs.primary, fontWeight: FontWeight.bold)),
        const SizedBox(width: 10),
        if (s.modelName.isNotEmpty)
          Flexible(
            child: Text(s.modelName,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(fontSize: 13, color: cs.secondary)),
          ),
        if (app.yolo) ...[
          const SizedBox(width: 8),
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
            decoration: BoxDecoration(
              color: cs.error.withValues(alpha: 0.2),
              borderRadius: BorderRadius.circular(6),
            ),
            child: Text('⚡ YOLO',
                style: TextStyle(
                    fontSize: 10, color: cs.error, fontWeight: FontWeight.bold)),
          ),
        ],
        if (s.busy) ...[
          const SizedBox(width: 10),
          const SizedBox(
              width: 14, height: 14, child: CircularProgressIndicator(strokeWidth: 2)),
        ],
      ]),
      actions: [
        IconButton(
            tooltip: 'Control center',
            onPressed: () => showControlCenter(context, app),
            icon: const Icon(Icons.tune)),
        IconButton(
            tooltip: 'New session',
            onPressed: app.newSession,
            icon: const Icon(Icons.add_comment_outlined)),
        if (showMenus)
          Builder(
            builder: (ctx) => IconButton(
                tooltip: 'Agent',
                onPressed: () => Scaffold.of(ctx).openEndDrawer(),
                icon: const Icon(Icons.insights_outlined)),
          ),
        IconButton(
            tooltip: 'Switch gateway',
            onPressed: app.disconnect,
            icon: const Icon(Icons.logout)),
      ],
    );
  }
}

/// The chat column: transcript + thinking/streaming + approval + composer.
class ChatPane extends StatefulWidget {
  final BlumiSession session;
  final VoidCallback? onCommand;
  const ChatPane(this.session, {this.onCommand, super.key});
  @override
  State<ChatPane> createState() => _ChatPaneState();
}

class _ChatPaneState extends State<ChatPane> {
  final _scroll = ScrollController();
  final _input = TextEditingController();

  @override
  void initState() {
    super.initState();
    widget.session.addListener(_autoscroll);
  }

  @override
  void dispose() {
    widget.session.removeListener(_autoscroll);
    _scroll.dispose();
    _input.dispose();
    super.dispose();
  }

  void _autoscroll() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scroll.hasClients) {
        _scroll.animateTo(_scroll.position.maxScrollExtent,
            duration: const Duration(milliseconds: 150), curve: Curves.easeOut);
      }
    });
  }

  void _send() {
    final t = _input.text;
    if (t.trim().isEmpty) return;
    _input.clear();
    widget.session.send(t);
  }

  bool _recording = false;

  /// Speak an assistant message via the gateway TTS (/api/voice/speak).
  Future<void> _speak(String text) async {
    final messenger = ScaffoldMessenger.of(context);
    try {
      await voice.play(await widget.session.api.speak(text));
    } catch (e) {
      messenger.showSnackBar(SnackBar(content: Text('voice: $e')));
    }
  }

  /// Mic toggle: start/stop recording, then transcribe into the composer.
  Future<void> _toggleMic() async {
    final messenger = ScaffoldMessenger.of(context);
    if (voice.recording) {
      final bytes = await voice.stop();
      if (mounted) setState(() => _recording = false);
      if (bytes == null || bytes.isEmpty) return;
      try {
        final text = await widget.session.api.transcribe(bytes);
        if (text.trim().isNotEmpty) {
          _input.text = '${_input.text} ${text.trim()}'.trim();
        }
      } catch (e) {
        messenger.showSnackBar(SnackBar(content: Text('voice: $e')));
      }
    } else {
      final ok = await voice.start();
      if (!ok) {
        messenger.showSnackBar(
            const SnackBar(content: Text('microphone permission denied')));
        return;
      }
      if (mounted) setState(() => _recording = true);
    }
  }

  @override
  Widget build(BuildContext context) {
    final s = widget.session;
    return AnimatedBuilder(
      animation: s,
      builder: (context, _) {
        final items = <Widget>[
          for (final e in s.entries) EntryView(e, speak: _speak),
          // Animated flower mascot while the agent is working but hasn't begun
          // streaming the answer (shows reasoning text if any).
          if (s.busy && s.streaming == null) ThinkingMascot(detail: s.thinking),
          if (s.streaming != null) EntryView(AssistantEntry(s.streaming!), streaming: true),
        ];
        return Column(
          children: [
            Expanded(
              child: s.switching
                  ? const Center(child: CircularProgressIndicator())
                  : items.isEmpty
                      ? const _EmptyState()
                      : ListView(
                          controller: _scroll,
                          padding: const EdgeInsets.all(12),
                          children: items,
                        ),
            ),
            if (s.pendingPlan != null) PlanCard(s, s.pendingPlan!),
            if (s.pendingApproval != null) ApprovalCard(s, s.pendingApproval!),
            if (s.pendingClarify != null) ClarifyCard(s, s.pendingClarify!),
            _Composer(
                input: _input,
                busy: s.busy,
                onSend: _send,
                onStop: s.cancel,
                onCommand: widget.onCommand,
                recording: _recording,
                onMic: _toggleMic),
          ],
        );
      },
    );
  }
}

class _EmptyState extends StatelessWidget {
  const _EmptyState();
  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Center(
      child: Column(mainAxisSize: MainAxisSize.min, children: [
        Text('✿', style: TextStyle(fontSize: 56, color: cs.primary)),
        const SizedBox(height: 8),
        Text('Ask blumi to build, fix, or explain…',
            style: TextStyle(color: cs.onSurface.withValues(alpha: 0.6))),
      ]),
    );
  }
}

class EntryView extends StatelessWidget {
  final Entry entry;
  final bool streaming;
  final void Function(String text)? speak;
  const EntryView(this.entry, {this.streaming = false, this.speak, super.key});

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return switch (entry) {
      UserEntry(:final text) => _Bubble(
          glyph: '›',
          label: 'you',
          color: cs.secondary,
          child: SelectableText(text),
        ),
      AssistantEntry(:final text) => _Bubble(
          glyph: '✿',
          label: streaming ? 'blumi…' : 'blumi',
          color: cs.primary,
          onSpeak: (!streaming && speak != null && text.trim().isNotEmpty)
              ? () => speak!(text)
              : null,
          child: BlumiMarkdown(text),
        ),
      NoticeEntry(:final text) => Padding(
          padding: const EdgeInsets.symmetric(vertical: 4),
          child: Text('· $text',
              style: TextStyle(
                  fontStyle: FontStyle.italic,
                  color: cs.onSurface.withValues(alpha: 0.5))),
        ),
      ToolEntry e => _ToolCard(e),
    };
  }
}

class _Bubble extends StatelessWidget {
  final String glyph, label;
  final Color color;
  final Widget child;
  final VoidCallback? onSpeak;
  const _Bubble(
      {required this.glyph,
      required this.label,
      required this.color,
      required this.child,
      this.onSpeak});

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Container(
      margin: const EdgeInsets.symmetric(vertical: 5),
      decoration: BoxDecoration(
        border: Border(left: BorderSide(color: color, width: 3)),
      ),
      padding: const EdgeInsets.only(left: 10, top: 4, bottom: 4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(children: [
            Text('$glyph $label',
                style: TextStyle(
                    color: color, fontWeight: FontWeight.bold, fontSize: 12)),
            if (onSpeak != null) ...[
              const Spacer(),
              InkWell(
                onTap: onSpeak,
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                  child: Icon(Icons.volume_up_outlined,
                      size: 16, color: cs.onSurface.withValues(alpha: 0.4)),
                ),
              ),
            ],
          ]),
          const SizedBox(height: 2),
          child,
        ],
      ),
    );
  }
}

class _ToolCard extends StatelessWidget {
  final ToolEntry e;
  const _ToolCard(this.e);
  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final (glyph, color) = switch (e.ok) {
      null => ('⠿', cs.secondary),
      true => ('✓', Colors.greenAccent),
      false => ('×', cs.error),
    };
    return Card(
      margin: const EdgeInsets.symmetric(vertical: 5),
      child: Padding(
        padding: const EdgeInsets.all(10),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(children: [
              Text('$glyph ', style: TextStyle(color: color)),
              Text('▸ ${e.name}',
                  style: const TextStyle(fontWeight: FontWeight.bold, fontFamily: 'monospace')),
            ]),
            if (e.summary.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: 2),
                child: Text(e.summary,
                    style: TextStyle(fontSize: 12, color: cs.onSurface.withValues(alpha: 0.7))),
              ),
            if (e.preview.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: 2),
                child: Text(e.preview,
                    maxLines: 6,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(fontSize: 12, color: cs.onSurface.withValues(alpha: 0.5))),
              ),
            if (e.diff != null) _DiffView(e.diff!),
          ],
        ),
      ),
    );
  }
}

class _DiffView extends StatelessWidget {
  final String diff;
  const _DiffView(this.diff);
  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final lines = diff.split('\n').take(40);
    return Container(
      margin: const EdgeInsets.only(top: 6),
      padding: const EdgeInsets.all(6),
      color: Colors.black.withValues(alpha: 0.25),
      width: double.infinity,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          for (final l in lines)
            Text(l,
                style: TextStyle(
                  fontFamily: 'monospace',
                  fontSize: 11,
                  color: l.startsWith('+') && !l.startsWith('+++')
                      ? Colors.greenAccent
                      : l.startsWith('-') && !l.startsWith('---')
                          ? cs.error
                          : l.startsWith('@')
                              ? cs.secondary
                              : cs.onSurface.withValues(alpha: 0.6),
                )),
        ],
      ),
    );
  }
}

class ApprovalCard extends StatelessWidget {
  final BlumiSession session;
  final ApprovalRequest req;
  const ApprovalCard(this.session, this.req, {super.key});
  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Container(
      width: double.infinity,
      margin: const EdgeInsets.all(8),
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: cs.surface,
        border: Border.all(color: req.dangerous ? cs.error : cs.primary),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(req.dangerous ? '⚠ permission — dangerous' : 'permission',
              style: TextStyle(
                  fontWeight: FontWeight.bold,
                  color: req.dangerous ? cs.error : cs.primary)),
          const SizedBox(height: 4),
          Text(req.tool, style: TextStyle(color: cs.secondary)),
          if (req.summary.isNotEmpty) Text(req.summary),
          if (req.advice != null)
            Padding(
              padding: const EdgeInsets.only(top: 4),
              child: Text(req.advice!, style: TextStyle(color: cs.secondary, fontSize: 12)),
            ),
          const SizedBox(height: 10),
          Wrap(spacing: 8, children: [
            FilledButton(
                onPressed: () => session.approve(allow: true),
                child: const Text('Allow once')),
            FilledButton.tonal(
                onPressed: () => session.approve(allow: true, session: true),
                child: const Text('Allow session')),
            OutlinedButton(
                onPressed: () => session.approve(allow: false),
                child: const Text('Deny')),
          ]),
        ],
      ),
    );
  }
}

class ClarifyCard extends StatelessWidget {
  final BlumiSession session;
  final ClarifyRequest req;
  const ClarifyCard(this.session, this.req, {super.key});
  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Container(
      width: double.infinity,
      margin: const EdgeInsets.all(8),
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: cs.surface,
        border: Border.all(color: cs.primary),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(req.question, style: const TextStyle(fontWeight: FontWeight.bold)),
          const SizedBox(height: 8),
          Wrap(
            spacing: 8,
            children: [
              for (final c in req.choices)
                OutlinedButton(
                    onPressed: () => session.answerClarify(c.id),
                    child: Text(c.label)),
            ],
          ),
        ],
      ),
    );
  }
}

/// A proposed plan awaiting review (the ExitPlanMode flow): scrollable markdown
/// plan with Approve (proceed) / Revise (reject) actions.
class PlanCard extends StatelessWidget {
  final BlumiSession session;
  final PlanReview req;
  const PlanCard(this.session, this.req, {super.key});
  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Container(
      width: double.infinity,
      margin: const EdgeInsets.all(8),
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: cs.surface,
        border: Border.all(color: cs.primary),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('✿ plan review',
              style: TextStyle(fontWeight: FontWeight.bold, color: cs.primary)),
          const SizedBox(height: 6),
          ConstrainedBox(
            constraints: const BoxConstraints(maxHeight: 340),
            child: SingleChildScrollView(child: BlumiMarkdown(req.plan)),
          ),
          const SizedBox(height: 10),
          Wrap(spacing: 8, children: [
            FilledButton(
                onPressed: () => session.answerPlan(true),
                child: const Text('Approve')),
            OutlinedButton(
                onPressed: () => session.answerPlan(false),
                child: const Text('Revise')),
          ]),
        ],
      ),
    );
  }
}

class _Composer extends StatelessWidget {
  final TextEditingController input;
  final bool busy;
  final VoidCallback onSend;
  final VoidCallback onStop;
  final VoidCallback? onCommand;
  final bool recording;
  final VoidCallback? onMic;
  const _Composer(
      {required this.input,
      required this.busy,
      required this.onSend,
      required this.onStop,
      this.onCommand,
      this.recording = false,
      this.onMic});

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(8, 4, 8, 8),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.end,
        children: [
          if (onMic != null)
            IconButton(
              tooltip: recording ? 'Stop recording' : 'Voice input',
              onPressed: onMic,
              icon: Icon(recording ? Icons.mic : Icons.mic_none,
                  color: recording ? Theme.of(context).colorScheme.error : null),
            ),
          Expanded(
            child: TextField(
              controller: input,
              minLines: 1,
              maxLines: 6,
              textInputAction: TextInputAction.send,
              onSubmitted: (_) => onSend(),
              onChanged: (v) {
                // Typing `/` on an empty composer opens the command palette.
                if (v == '/' && onCommand != null) {
                  input.clear();
                  onCommand!();
                }
              },
              decoration: const InputDecoration(
                hintText: 'Ask blumi…  (/ for commands)',
                border: OutlineInputBorder(),
                isDense: true,
              ),
            ),
          ),
          const SizedBox(width: 8),
          busy
              ? IconButton.filledTonal(
                  onPressed: onStop, icon: const Icon(Icons.stop))
              : IconButton.filled(onPressed: onSend, icon: const Icon(Icons.send)),
        ],
      ),
    );
  }
}

/// Right "agent" rail: live metrics + tasks + active sub-agents (TUI parity).
class AgentRail extends StatelessWidget {
  final BlumiSession session;
  const AgentRail(this.session, {super.key});
  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return AnimatedBuilder(
      animation: session,
      builder: (context, _) {
        final s = session;
        return ListView(
          padding: const EdgeInsets.all(12),
          children: [
            Text('● agent',
                style: TextStyle(fontWeight: FontWeight.bold, color: cs.primary)),
            const SizedBox(height: 8),
            _meter(context, 'context', s.contextFrac,
                '${(s.contextFrac * 100).round()}%'),
            const SizedBox(height: 8),
            _kv('tokens', '↑${s.inputTokens} ↓${s.outputTokens}'),
            if (s.costUsd > 0) _kv('cost', '\$${s.costUsd.toStringAsFixed(4)}'),
            const Divider(),
            Text('tasks', style: TextStyle(fontWeight: FontWeight.bold, color: cs.secondary)),
            if (s.todos.isEmpty)
              Text('(none yet)',
                  style: TextStyle(color: cs.onSurface.withValues(alpha: 0.5))),
            for (final t in s.todos)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 2),
                child: Row(children: [
                  Text(switch (t.status) {
                    TodoStatus.completed => '✓ ',
                    TodoStatus.inProgress => '◐ ',
                    TodoStatus.pending => '• ',
                  }),
                  Expanded(child: Text(t.content, style: const TextStyle(fontSize: 13))),
                ]),
              ),
            if (s.agents.isNotEmpty) ...[
              const Divider(),
              Text('active agents',
                  style: TextStyle(fontWeight: FontWeight.bold, color: cs.secondary)),
              for (final a in s.agents)
                ListTile(
                  dense: true,
                  contentPadding: EdgeInsets.zero,
                  leading: Text(switch (a.status) {
                    AgentStatus.working => '⠿',
                    AgentStatus.done => '✓',
                    AgentStatus.failed => '×',
                  }),
                  title: Text(a.role),
                  subtitle: Text(a.task, maxLines: 1, overflow: TextOverflow.ellipsis),
                ),
            ],
          ],
        );
      },
    );
  }

  Widget _kv(String k, String v) => Padding(
        padding: const EdgeInsets.symmetric(vertical: 2),
        child: Row(mainAxisAlignment: MainAxisAlignment.spaceBetween, children: [
          Text(k, style: const TextStyle(fontSize: 13)),
          Text(v, style: const TextStyle(fontSize: 13, fontFamily: 'monospace')),
        ]),
      );

  Widget _meter(BuildContext context, String label, double frac, String pct) {
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Row(mainAxisAlignment: MainAxisAlignment.spaceBetween, children: [
        Text(label, style: const TextStyle(fontSize: 13)),
        Text(pct, style: const TextStyle(fontSize: 13)),
      ]),
      const SizedBox(height: 3),
      ClipRRect(
        borderRadius: BorderRadius.circular(4),
        child: LinearProgressIndicator(value: frac, minHeight: 6),
      ),
    ]);
  }
}

/// Left "explorer" rail: the sessions list + new/refresh (TUI parity).
class SessionsPane extends StatelessWidget {
  final AppController app;
  const SessionsPane(this.app, {super.key});
  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return AnimatedBuilder(
      animation: app,
      builder: (context, _) => Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 12, 8, 4),
            child: Row(children: [
              Text('explorer',
                  style: TextStyle(fontWeight: FontWeight.bold, color: cs.primary)),
              const Spacer(),
              IconButton(
                  tooltip: 'Refresh',
                  onPressed: app.refreshSessions,
                  icon: const Icon(Icons.refresh, size: 18)),
            ]),
          ),
          Expanded(
            child: app.sessions.isEmpty
                ? Center(
                    child: Text('(no sessions)',
                        style: TextStyle(color: cs.onSurface.withValues(alpha: 0.5))))
                : ListView(
                    children: [
                      for (final sess in app.sessions)
                        ListTile(
                          dense: true,
                          title: Text(
                            sess.title.isEmpty ? '(untitled)' : sess.title,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                          subtitle: Text('${sess.messageCount} msgs',
                              style: const TextStyle(fontSize: 11)),
                          onTap: () {
                            final sc = Scaffold.maybeOf(context);
                            if (sc?.isDrawerOpen ?? false) sc!.closeDrawer();
                            app.resumeSession(sess.id);
                          },
                        ),
                    ],
                  ),
          ),
        ],
      ),
    );
  }
}
