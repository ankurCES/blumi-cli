import 'dart:async';
import 'package:flutter/foundation.dart';
import '../data/api.dart';
import '../data/events.dart';
import '../data/models.dart';
import '../data/sse.dart';

enum AgentStatus { working, done, failed }

class AgentCardVM {
  final String id, role, task;
  AgentStatus status;
  AgentCardVM(
      {required this.id,
      required this.role,
      required this.task,
      required this.status});
}

/// The live mirror of one blumi session — the mobile analog of the TUI's `Model`.
/// Consumes the SSE event stream and exposes transcript + run state for the UI.
class BlumiSession extends ChangeNotifier {
  final ApiClient api;
  final EventStream _stream;
  StreamSubscription<BlumiEvent>? _sub;

  final List<Entry> entries = [];
  String? streaming; // in-flight assistant text
  String? thinking; // in-flight reasoning
  bool busy = false;
  String modelName = '';
  List<Todo> todos = [];
  int contextTokens = 0, contextSize = 1, inputTokens = 0, outputTokens = 0;
  double costUsd = 0;
  ApprovalRequest? pendingApproval;
  ClarifyRequest? pendingClarify;
  PlanReview? pendingPlan;
  final List<AgentCardVM> agents = [];

  BlumiSession(ServerConn conn)
      : api = ApiClient(conn),
        _stream = EventStream(conn);

  Future<void> start() async {
    await restore();
    _sub = _stream.connect().listen(_onEvent);
    notifyListeners();
  }

  /// (Re)load the server's config + current transcript — called on connect and
  /// after a session switch/new/resume.
  Future<void> restore() async {
    try {
      final cfg = await api.config();
      contextSize = (cfg['context_size'] as num?)?.toInt() ?? contextSize;
      modelName = cfg['model'] as String? ?? modelName;
    } catch (_) {}
    try {
      final msgs = await api.messages();
      entries
        ..clear()
        ..addAll(msgs.map((m) => switch (m.role) {
              'user' => UserEntry(m.text),
              'tool' => ToolEntry(
                  id: '', name: m.toolName ?? 'tool', summary: m.text, ok: true),
              _ => AssistantEntry(m.text),
            }));
    } catch (_) {}
    streaming = null;
    thinking = null;
    busy = false;
    notifyListeners();
  }

  void _onEvent(BlumiEvent ev) {
    switch (ev) {
      case TurnStarted():
        busy = true;
      case AssistantStarted():
        streaming = '';
      case TokenEvent(:final text):
        streaming = (streaming ?? '') + text;
      case ThinkingEvent(:final text):
        thinking = (thinking ?? '') + text;
      case AssistantFinished():
        if (streaming != null && streaming!.trim().isNotEmpty) {
          entries.add(AssistantEntry(streaming!));
        }
        streaming = null;
        thinking = null;
      case ToolStart(:final id, :final name, :final summary):
        entries.add(ToolEntry(id: id, name: name, summary: summary));
      case ToolResultEvent(:final id, :final ok, :final preview):
        final t = _tool(id);
        if (t != null) {
          t.ok = ok;
          if (preview.isNotEmpty) t.preview = preview;
        }
      case DiffEvent(:final id, :final unified):
        _tool(id)?.diff = unified;
      case ApprovalRequest():
        pendingApproval = ev;
      case ClarifyRequest():
        pendingClarify = ev;
      case PlanReview():
        pendingPlan = ev;
      case TodoUpdate(:final items):
        todos = items;
      case UsageEvent(:final input, :final output, :final context, :final costUsd):
        inputTokens = input;
        outputTokens = output;
        if (context > 0) contextTokens = context;
        if (costUsd != null) this.costUsd = costUsd;
      case AgentStart(:final id, :final agentType, :final task):
        agents.add(AgentCardVM(
            id: id, role: agentType, task: task, status: AgentStatus.working));
      case AgentDone(:final id, :final ok):
        for (final a in agents) {
          if (a.id == id) a.status = ok ? AgentStatus.done : AgentStatus.failed;
        }
      case DoneEvent():
        busy = false;
        streaming = null;
        thinking = null;
      case NoticeEvent(:final message):
        entries.add(NoticeEntry(message));
      case ErrorEvent(:final message):
        busy = false;
        entries.add(NoticeEntry('error: $message'));
      case Compaction():
        contextTokens = 0;
        entries.add(const NoticeEntry('context compacted'));
      case _:
        break;
    }
    notifyListeners();
  }

  ToolEntry? _tool(String id) {
    for (final e in entries.reversed) {
      if (e is ToolEntry && e.id == id) return e;
    }
    return null;
  }

  Future<void> send(String text) async {
    if (text.trim().isEmpty) return;
    entries.add(UserEntry(text));
    busy = true;
    notifyListeners();
    try {
      await api.send(text);
    } catch (e) {
      busy = false;
      entries.add(NoticeEntry('send failed: $e'));
      notifyListeners();
    }
  }

  Future<void> cancel() async {
    try {
      await api.cancel();
    } catch (_) {}
  }

  Future<void> approve({required bool allow, bool session = false}) async {
    final req = pendingApproval;
    if (req == null) return;
    pendingApproval = null;
    notifyListeners();
    try {
      await api.approve(req.requestId, allow: allow, session: session);
    } catch (_) {}
  }

  Future<void> answerClarify(String value) async {
    final req = pendingClarify;
    if (req == null) return;
    pendingClarify = null;
    notifyListeners();
    try {
      await api.clarify(req.requestId, value);
    } catch (_) {}
  }

  double get contextFrac =>
      contextSize > 0 ? (contextTokens / contextSize).clamp(0.0, 1.0) : 0.0;

  @override
  void dispose() {
    _sub?.cancel();
    _stream.close();
    super.dispose();
  }
}
