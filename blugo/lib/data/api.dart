import 'dart:convert';
import 'package:http/http.dart' as http;
import 'models.dart';

/// Where + how to reach a gateway: base URL + an optional Bearer token.
class ServerConn {
  final String baseUrl; // e.g. http://10.0.0.61:7777
  final String? token;
  const ServerConn(this.baseUrl, [this.token]);
  ServerConn withToken(String? t) => ServerConn(baseUrl, t);
}

class ApiException implements Exception {
  final String message;
  ApiException(this.message);
  @override
  String toString() => message;
}

/// A selectable agent persona (architect, pair, reviewer, …).
class PersonaInfo {
  final String name, description;
  const PersonaInfo(this.name, this.description);
}

/// One item on the persistent task board (`blumi-task`).
class TaskItem {
  final String id, title, detail, state;
  final int priority;

  /// The grid peer executing this task (its display name), or null when it runs
  /// locally. Set by the orchestrator when a task is handed off.
  final String? owner;
  const TaskItem({
    required this.id,
    required this.title,
    required this.detail,
    required this.state,
    required this.priority,
    this.owner,
  });

  factory TaskItem.fromMap(Map<String, dynamic> j) {
    final owner = j['owner']?.toString();
    return TaskItem(
      id: j['id']?.toString() ?? '',
      title: j['title']?.toString() ?? '',
      detail: j['detail']?.toString() ?? '',
      state: j['state']?.toString() ?? 'todo',
      priority: (j['priority'] as num?)?.toInt() ?? 3,
      owner: (owner != null && owner.isNotEmpty) ? owner : null,
    );
  }
}

/// A discovered grid peer (from `GET /api/grid/peers`).
class GridPeer {
  final String id, name, host, version, gridId;
  final int port;
  final bool online;
  const GridPeer({
    required this.id,
    required this.name,
    required this.host,
    required this.port,
    required this.version,
    required this.gridId,
    required this.online,
  });

  factory GridPeer.fromMap(Map<String, dynamic> j) => GridPeer(
        id: j['id']?.toString() ?? '',
        name: j['name']?.toString() ?? '',
        host: j['host']?.toString() ?? '',
        port: (j['port'] as num?)?.toInt() ?? 0,
        version: j['version']?.toString() ?? '',
        gridId: j['grid_id']?.toString() ?? '',
        online: j['online'] as bool? ?? false,
      );
}

/// REST client for the blumi gateway. Mirrors the endpoints the web UI uses.
class ApiClient {
  final ServerConn conn;
  final http.Client _http;
  ApiClient(this.conn, [http.Client? client]) : _http = client ?? http.Client();

  Map<String, String> _headers({bool json = true}) => {
        if (json) 'Content-Type': 'application/json',
        if (conn.token != null) 'Authorization': 'Bearer ${conn.token}',
      };

  Uri _u(String path) => Uri.parse('${conn.baseUrl}$path');

  /// Returns the auth token on success (null when the server has auth disabled);
  /// throws on a bad password or unreachable host.
  Future<String?> login(String password) async {
    final r = await _http.post(_u('/api/login'),
        headers: _headers(), body: jsonEncode({'password': password}));
    if (r.statusCode == 200) {
      return (jsonDecode(r.body) as Map<String, dynamic>)['token'] as String?;
    }
    throw ApiException('login failed (HTTP ${r.statusCode})');
  }

  Future<Map<String, dynamic>> config() => _getJson('/api/config');

  Future<List<SessionInfo>> sessions() async {
    final j = await _getJson('/api/sessions');
    return ((j['sessions'] as List?) ?? [])
        .map((s) => SessionInfo.fromMap(s as Map<String, dynamic>))
        .toList();
  }

  Future<List<StoredMessage>> messages() async {
    final j = await _getJson('/api/messages');
    return ((j['messages'] as List?) ?? [])
        .map((m) => StoredMessage.fromMap(m as Map<String, dynamic>))
        .toList();
  }

  Future<void> send(String text) => _post('/api/chat/send', {'text': text});
  Future<void> cancel() => _post('/api/chat/cancel', const {});
  Future<void> newSession() => _post('/api/session/new', const {});
  Future<void> resume(String id) => _post('/api/session/resume', {'id': id});
  Future<void> setYolo(bool on) => _post('/api/yolo', {'on': on});
  Future<void> setPlanMode(bool on) => _post('/api/plan/mode', {'on': on});
  Future<void> setBrainMode(String mode) =>
      _post('/api/brain/mode', {'mode': mode});
  Future<void> setAutoContinue(int n) => _post('/api/autocontinue', {'n': n});
  Future<void> compact() => _post('/api/compact', const {});
  Future<void> undo() => _post('/api/undo', const {});

  Future<void> approve(String requestId,
          {required bool allow, bool session = false}) =>
      _post('/api/approval/respond', {
        'request_id': requestId,
        'decision': allow ? 'allow' : 'deny',
        'scope': session ? 'session' : 'once',
      });

  Future<void> clarify(String requestId, String value) =>
      _post('/api/clarify/respond', {'request_id': requestId, 'value': value});

  // --- control center --------------------------------------------------------

  Future<List<String>> models() async {
    final j = await _getJson('/api/models');
    return ((j['options'] as List?) ?? [])
        .map((o) => o is Map ? (o['id'] ?? o['name'] ?? '$o').toString() : '$o')
        .where((s) => s.isNotEmpty)
        .toList();
  }

  Future<void> setModel(String model) => _post('/api/model/set', {'model': model});

  /// (personas, activeName).
  Future<(List<PersonaInfo>, String)> personas() async {
    final j = await _getJson('/api/personas');
    final list = ((j['personas'] as List?) ?? [])
        .map((p) => PersonaInfo(
              (p as Map)['name']?.toString() ?? '',
              p['description']?.toString() ?? '',
            ))
        .toList();
    return (list, j['active']?.toString() ?? '');
  }

  Future<void> setPersona(String name) => _post('/api/persona/set', {'name': name});

  Future<List<String>> skills() async {
    final j = await _getJson('/api/skills');
    return ((j['skills'] as List?) ?? [])
        .map((s) => s is Map ? (s['name'] ?? '$s').toString() : '$s')
        .toList();
  }

  Future<List<TaskItem>> tasks() async {
    final j = await _getJson('/api/tasks');
    return ((j['tasks'] as List?) ?? [])
        .map((t) => TaskItem.fromMap(t as Map<String, dynamic>))
        .toList();
  }

  Future<Map<String, dynamic>> loopStatus() => _getJson('/api/loop/status');
  Future<void> loopStart({bool review = false}) =>
      _post('/api/loop/start', {'review': review});
  Future<void> loopStop() => _post('/api/loop/stop', const {});

  // --- voice -----------------------------------------------------------------

  /// Transcribe recorded audio (raw bytes + its mime) → text.
  Future<String> transcribe(List<int> audio, {String mime = 'audio/m4a'}) async {
    final r = await _http.post(
      _u('/api/voice/transcribe'),
      headers: {
        'Content-Type': mime,
        if (conn.token != null) 'Authorization': 'Bearer ${conn.token}',
      },
      body: audio,
    );
    if (r.statusCode != 200) throw ApiException('transcribe → ${r.statusCode}');
    return (jsonDecode(r.body) as Map<String, dynamic>)['text'] as String? ?? '';
  }

  /// Synthesize speech for [text] → audio bytes (mp3).
  Future<List<int>> speak(String text) async {
    final r = await _http.post(_u('/api/voice/speak'),
        headers: _headers(), body: jsonEncode({'text': text}));
    if (r.statusCode != 200) throw ApiException('speak → ${r.statusCode}');
    return r.bodyBytes;
  }

  Future<Map<String, dynamic>> usage() async {
    final j = await _getJson('/api/usage');
    return (j['usage'] as Map?)?.cast<String, dynamic>() ?? {};
  }

  Future<Map<String, dynamic>> status() => _getJson('/api/status');

  Future<Map<String, dynamic>> settings() => _getJson('/api/settings');
  Future<void> setSettings(Map<String, dynamic> patch) =>
      _post('/api/settings', patch);

  /// (projectMemory, userMemory).
  Future<(String, String)> memory() async {
    final j = await _getJson('/api/memory');
    return (j['memory']?.toString() ?? '', j['user']?.toString() ?? '');
  }

  Future<void> setMemory(String which, String content) =>
      _post('/api/memory', {'which': which, 'content': content});

  // --- Grid (distributed) ---

  /// Discovered grid peers: `{ self: {...}, peers: [...] }` (or disabled).
  Future<(List<GridPeer>, Map<String, dynamic>)> gridPeers() async {
    final j = await _getJson('/api/grid/peers');
    final peers = ((j['peers'] as List?) ?? [])
        .map((p) => GridPeer.fromMap(p as Map<String, dynamic>))
        .toList();
    final me = (j['self'] as Map<String, dynamic>?) ?? const {};
    return (peers, me);
  }

  /// Aggregated grid metrics: `{ self, peers:[{name,online,metrics}], totals }`.
  Future<Map<String, dynamic>> gridMetrics() => _getJson('/api/grid/metrics');

  /// Hand a board task off to a grid peer for remote execution.
  Future<Map<String, dynamic>> gridDispatch(String taskId, String peerId,
          {bool review = false}) =>
      _postJson('/api/grid/dispatch',
          {'task_id': taskId, 'peer_id': peerId, 'review': review});

  /// Delegate a free-form prompt over the grid (deterministic — no model
  /// tool-call). `target` = 'all' (broadcast to every live peer) or a peer
  /// name/host. Returns `{ ok, results: [{peer, host, ok, output|error, ms}] }`.
  Future<Map<String, dynamic>> gridDelegate(String prompt,
          {String target = 'all'}) =>
      _postJson('/api/grid/delegate', {'prompt': prompt, 'target': target});

  // --- Knowledge base / memory (UI) ---

  /// Code-KB totals + ingest-job state: `{ enabled, files, symbols, vectors,
  /// sources, ingesting, message }`.
  Future<Map<String, dynamic>> knowledgeStatus() =>
      _getJson('/api/knowledge/status');

  /// Indexed sources: `{ sources: [{ source, files, symbols }] }`.
  Future<Map<String, dynamic>> knowledgeSources() =>
      _getJson('/api/knowledge/sources');

  /// Hybrid code search: `{ hits: [{ path, name, kind, start_line, snippet }] }`.
  Future<Map<String, dynamic>> knowledgeSearch(String query, {int limit = 10}) =>
      _postJson('/api/knowledge/search', {'query': query, 'limit': limit});

  /// Start a background ingest of `path`. Poll [knowledgeStatus] for progress.
  Future<Map<String, dynamic>> knowledgeIngest(String path) =>
      _postJson('/api/knowledge/ingest', {'path': path});

  /// Remove an indexed source by its label.
  Future<Map<String, dynamic>> knowledgeRemove(String source) =>
      _postJson('/api/knowledge/remove', {'source': source});

  /// Semantic search over long-term memory: `{ hits: [{ namespace, text }] }`.
  Future<Map<String, dynamic>> memorySearch(String query, {int limit = 10}) =>
      _postJson('/api/memory/search', {'query': query, 'limit': limit});

  /// Proposed-plan history: `[{ title, content, status, created_at }]`
  /// (status: live | approved | rejected), newest last.
  Future<List<Map<String, dynamic>>> plans() async {
    final j = await _getJson('/api/plans');
    return ((j['plans'] as List?) ?? []).cast<Map<String, dynamic>>();
  }

  // --- Self-management ---

  /// Reload the agent in place (apply config/skill changes).
  Future<void> selfReload() => _post('/api/self/reload', const {});

  /// Restart the whole gateway service (requires confirm).
  Future<Map<String, dynamic>> selfRestart() =>
      _postJson('/api/self/restart', {'confirm': true});

  /// Try to recover a wedged gateway (reload, escalating to restart).
  Future<Map<String, dynamic>> selfRecover() =>
      _postJson('/api/self/recover', const {});

  /// settings.json with secrets redacted: `{ settings: {...} }`.
  Future<Map<String, dynamic>> selfConfigGet() => _getJson('/api/self/config');

  /// Set one dotted config key (validated server-side), optionally reloading.
  Future<Map<String, dynamic>> selfConfigSet(String key, String value,
          {bool reload = false}) =>
      _postJson('/api/self/config',
          {'key': key, 'value': value, 'reload': reload});

  /// Create/update a skill, optionally reloading to load it.
  Future<Map<String, dynamic>> skillWrite(
          String name, String description, String instructions,
          {bool reload = false}) =>
      _postJson('/api/skills', {
        'name': name,
        'description': description,
        'instructions': instructions,
        'reload': reload,
      });

  /// Delete a skill by name.
  Future<Map<String, dynamic>> skillDelete(String name) =>
      _postJson('/api/skills/delete', {'name': name});

  /// Raw GET of a JSON endpoint (used by the cache layer).
  Future<Map<String, dynamic>> getJson(String path) => _getJson(path);

  Future<Map<String, dynamic>> _getJson(String path) async {
    final r = await _http.get(_u(path), headers: _headers(json: false));
    if (r.statusCode != 200) throw ApiException('GET $path → ${r.statusCode}');
    return jsonDecode(r.body) as Map<String, dynamic>;
  }

  Future<void> _post(String path, Object body) async {
    final r =
        await _http.post(_u(path), headers: _headers(), body: jsonEncode(body));
    if (r.statusCode != 200) throw ApiException('POST $path → ${r.statusCode}');
  }

  /// POST returning the JSON body (for actions that report a result).
  Future<Map<String, dynamic>> _postJson(String path, Object body) async {
    final r =
        await _http.post(_u(path), headers: _headers(), body: jsonEncode(body));
    if (r.statusCode != 200) throw ApiException('POST $path → ${r.statusCode}');
    return jsonDecode(r.body) as Map<String, dynamic>;
  }
}
