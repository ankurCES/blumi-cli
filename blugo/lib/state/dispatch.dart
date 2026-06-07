import 'dart:async';
import 'package:flutter/foundation.dart';
import '../data/api.dart';
import '../data/models.dart';
import '../data/saved_server.dart';
import 'app.dart';
import 'session.dart';

/// Stable per-gateway dispatch session id. Each saved gateway has exactly one
/// dispatch thread, isolated from its workbench session (the gateway lazily
/// creates it via `create_with_id`).
const String kDispatchSessionId = 'dispatch';

/// One node's reply within a broadcast turn.
class NodeReply {
  final String node;
  final String? text; // set on success
  final String? error; // set on failure
  final bool pending;
  const NodeReply._(this.node, this.text, this.error, this.pending);
  factory NodeReply.pending(String node) => NodeReply._(node, null, null, true);
  factory NodeReply.ok(String node, String text) =>
      NodeReply._(node, text, null, false);
  factory NodeReply.error(String node, String error) =>
      NodeReply._(node, null, error, false);
}

/// One fan-out: a prompt sent to every saved gateway + each node's reply.
class BroadcastTurn {
  final String prompt;
  final Map<String, NodeReply> replies = {}; // serverId → reply
  BroadcastTurn(this.prompt);
}

/// Owns the Telegram-style dispatch threads: one isolated [BlumiSession] per
/// saved gateway (id [kDispatchSessionId], FCM-backed, no local double-ping),
/// plus the phone-side broadcast fan-out across all saved gateways.
class DispatchController extends ChangeNotifier {
  final AppController app;
  DispatchController(this.app);

  final Map<String, BlumiSession> _sessions = {}; // serverId → dispatch session
  final Map<String, Future<void>> _started = {}; // serverId → initial start()

  final List<BroadcastTurn> broadcastTurns = [];

  /// The dispatch session for [s], created (and started) on first use. Isolated
  /// from the gateway's workbench session.
  BlumiSession openFor(SavedServer s) {
    final existing = _sessions[s.id];
    if (existing != null) return existing;
    final session = BlumiSession(
      ServerConn(s.base, s.token),
      sessionId: kDispatchSessionId,
      notifyTitle: s.name,
      localNotify: false, // FCM handles backgrounded dispatch pings
    );
    _sessions[s.id] = session;
    _started[s.id] = session.start();
    unawaited(app.registerFcmForServer(s)); // ensure this node can push us
    notifyListeners();
    return session;
  }

  Future<BlumiSession> _readyFor(SavedServer s) async {
    final session = openFor(s);
    await _started[s.id];
    return session;
  }

  /// Broadcast [text] to every saved gateway in parallel, collecting each node's
  /// reply (with a per-node timeout so one slow/offline node can't block).
  Future<void> broadcast(String text) async {
    if (text.trim().isEmpty) return;
    final turn = BroadcastTurn(text);
    for (final s in app.servers) {
      turn.replies[s.id] = NodeReply.pending(s.name);
    }
    broadcastTurns.add(turn);
    notifyListeners();
    await Future.wait(app.servers.map((s) => _broadcastTo(s, text, turn)));
  }

  Future<void> _broadcastTo(
      SavedServer s, String text, BroadcastTurn turn) async {
    if (s.token == null) {
      turn.replies[s.id] = NodeReply.error(s.name, 'not signed in');
      notifyListeners();
      return;
    }
    try {
      final reply = await _oneShot(s, text)
          .timeout(const Duration(seconds: 180));
      turn.replies[s.id] = NodeReply.ok(s.name, reply);
    } catch (e) {
      turn.replies[s.id] = NodeReply.error(s.name, '$e');
    }
    notifyListeners();
  }

  /// Send [text] to a node's dispatch session and resolve with its reply once
  /// the turn finishes (busy true→false), falling back to the canonical
  /// transcript when the live tail is empty (e.g. SSE attached mid-turn).
  Future<String> _oneShot(SavedServer s, String text) async {
    final session = await _readyFor(s);
    final completer = Completer<String>();
    var sawBusy = false;
    void listener() {
      if (session.busy) {
        sawBusy = true;
        return;
      }
      if (sawBusy && !completer.isCompleted) {
        () async {
          var reply = _lastAssistant(session);
          if (reply.isEmpty) {
            try {
              final msgs =
                  await session.api.messages(sessionId: kDispatchSessionId);
              for (final m in msgs.reversed) {
                if (m.role == 'assistant' && m.text.trim().isNotEmpty) {
                  reply = m.text;
                  break;
                }
              }
            } catch (_) {}
          }
          if (!completer.isCompleted) completer.complete(reply);
        }();
      }
    }

    session.addListener(listener);
    try {
      await session.send(text);
      return await completer.future;
    } finally {
      session.removeListener(listener);
    }
  }

  String _lastAssistant(BlumiSession session) {
    for (final e in session.entries.reversed) {
      if (e is AssistantEntry && e.text.trim().isNotEmpty) return e.text;
    }
    return '';
  }

  @override
  void dispose() {
    for (final s in _sessions.values) {
      s.dispose();
    }
    _sessions.clear();
    super.dispose();
  }
}
