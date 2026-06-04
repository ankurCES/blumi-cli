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

  Future<Map<String, dynamic>> usage() async {
    final j = await _getJson('/api/usage');
    return (j['usage'] as Map?)?.cast<String, dynamic>() ?? {};
  }

  /// (projectMemory, userMemory).
  Future<(String, String)> memory() async {
    final j = await _getJson('/api/memory');
    return (j['memory']?.toString() ?? '', j['user']?.toString() ?? '');
  }

  Future<void> setMemory(String which, String content) =>
      _post('/api/memory', {'which': which, 'content': content});

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
}
