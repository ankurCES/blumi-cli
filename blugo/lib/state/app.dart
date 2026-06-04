import 'dart:convert';
import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';
import '../data/api.dart';
import '../data/discovery.dart';
import '../data/models.dart';
import '../data/saved_server.dart';
import 'session.dart';

/// Top-level controller: owns the saved-server list, the live connection +
/// [BlumiSession], login/persistence, and the sessions list. blugo can store
/// several gateways (one per machine) and switch between them.
class AppController extends ChangeNotifier {
  BlumiSession? session;
  ServerConn? conn;
  String status = '';
  bool connecting = false;
  List<SessionInfo> sessions = [];

  /// Saved gateways, newest last. Persisted under `servers`.
  List<SavedServer> servers = [];

  /// The server currently connected (or being connected).
  String? currentServerId;

  /// When set, the connect screen shows a password prompt for this server
  /// (its saved token was missing or rejected).
  SavedServer? reauthFor;

  /// All gateways found on the LAN via mDNS (raw). [discovered] filters out the
  /// ones already saved, recomputed live so forgetting a server re-surfaces it.
  List<SavedServer> _discoveredRaw = [];
  LanDiscovery? _lan;

  List<SavedServer> get discovered {
    final saved = servers.map((s) => s.id).toSet();
    return _discoveredRaw.where((s) => !saved.contains(s.id)).toList();
  }

  /// Selected UI theme (persisted) + runtime auto-approve (yolo) toggle.
  String themeName = 'rose';
  bool yolo = false;

  bool get connected => session != null;

  static const _kServers = 'servers';
  static const _kLast = 'lastServerId';

  // --- persistence -----------------------------------------------------------

  Future<void> _loadServers() async {
    final p = await SharedPreferences.getInstance();
    themeName = p.getString('theme') ?? themeName;
    final raw = p.getString(_kServers);
    if (raw != null) {
      try {
        servers = (jsonDecode(raw) as List)
            .map((e) => SavedServer.fromJson(e as Map<String, dynamic>))
            .toList();
      } catch (_) {
        servers = [];
      }
    }
    // Migrate the pre-multi-server single connection, if any.
    if (servers.isEmpty) {
      final base = p.getString('baseUrl');
      if (base != null) {
        final (host, port) = _splitBase(base);
        servers = [
          SavedServer.create(
              name: host, host: host, port: port, token: p.getString('token')),
        ];
        await _saveServers();
        await p.remove('baseUrl');
        await p.remove('token');
      }
    }
  }

  Future<void> _saveServers() async {
    final p = await SharedPreferences.getInstance();
    await p.setString(
        _kServers, jsonEncode(servers.map((s) => s.toJson()).toList()));
  }

  Future<void> _rememberLast(String id) async {
    final p = await SharedPreferences.getInstance();
    await p.setString(_kLast, id);
  }

  // --- connect flows ---------------------------------------------------------

  /// On launch: load saved servers and silently reconnect to the last one if it
  /// still has a usable token. Returns true once a session is live.
  Future<bool> tryAutoConnect() async {
    await _loadServers();
    notifyListeners();
    if (servers.isEmpty) return false;
    final p = await SharedPreferences.getInstance();
    final lastId = p.getString(_kLast);
    final srv = _byId(lastId) ?? servers.first;
    if (srv.token == null) return false;
    await connectToSaved(srv);
    return connected;
  }

  /// Connect to a saved server using its stored token. If the token is missing
  /// or rejected, fall back to a password prompt ([reauthFor]).
  Future<void> connectToSaved(SavedServer srv) async {
    connecting = true;
    status = 'connecting to ${srv.name}…';
    reauthFor = null;
    notifyListeners();
    if (srv.token == null) {
      connecting = false;
      reauthFor = srv;
      status = '';
      notifyListeners();
      return;
    }
    final c = ServerConn(srv.base, srv.token);
    try {
      await ApiClient(c).config(); // probe auth with the saved token
      conn = c;
      currentServerId = srv.id;
      await _rememberLast(srv.id);
      await _open();
    } catch (_) {
      // Token stale/rejected → ask for the password for this server.
      connecting = false;
      reauthFor = srv;
      status = '';
      notifyListeners();
    }
  }

  /// Add (or update) a server and connect. A blank [name] defaults to the
  /// gateway's machine hostname (from `/api/config`), else the host.
  Future<void> addAndConnect({
    String? name,
    required String host,
    required int port,
    required String password,
  }) async {
    connecting = true;
    status = 'connecting…';
    notifyListeners();
    final base = host.startsWith('http') ? host : 'http://$host:$port';
    try {
      final token = await ApiClient(ServerConn(base)).login(password);
      final c = ServerConn(base, token);

      var label = (name ?? '').trim();
      if (label.isEmpty) {
        try {
          final cfg = await ApiClient(c).config();
          label = (cfg['hostname'] as String?)?.trim() ?? '';
        } catch (_) {}
      }
      if (label.isEmpty) label = host;

      final srv =
          SavedServer.create(name: label, host: host, port: port, token: token);
      servers = [...servers.where((s) => s.id != srv.id), srv];
      await _saveServers();

      conn = c;
      currentServerId = srv.id;
      reauthFor = null;
      await _rememberLast(srv.id);
      await _open();
    } catch (e) {
      status = '$e';
      connecting = false;
      notifyListeners();
    }
  }

  /// Re-authenticate a saved server whose token went stale, keeping its name.
  Future<void> reauthenticate(String password) async {
    final srv = reauthFor;
    if (srv == null) return;
    await addAndConnect(
        name: srv.name, host: srv.host, port: srv.port, password: password);
  }

  void cancelReauth() {
    reauthFor = null;
    status = '';
    notifyListeners();
  }

  Future<void> removeServer(String id) async {
    servers = servers.where((s) => s.id != id).toList();
    if (reauthFor?.id == id) reauthFor = null;
    await _saveServers();
    notifyListeners();
  }

  // --- LAN discovery (mDNS) --------------------------------------------------

  /// Browse the LAN for `_blumi._tcp` beacons; updates [discovered] live.
  /// Hides any gateway already saved. Safe to call repeatedly.
  Future<void> startDiscovery() async {
    if (_lan != null) return;
    _lan = LanDiscovery((found) {
      _discoveredRaw = found;
      notifyListeners();
    });
    await _lan!.start();
  }

  Future<void> stopDiscovery() async {
    final l = _lan;
    _lan = null;
    if (_discoveredRaw.isNotEmpty) {
      _discoveredRaw = [];
      notifyListeners();
    }
    await l?.stop();
  }

  // --- control center --------------------------------------------------------

  Future<void> setTheme(String name) async {
    themeName = name;
    notifyListeners();
    final p = await SharedPreferences.getInstance();
    await p.setString('theme', name);
  }

  Future<void> setYolo(bool on) async {
    yolo = on;
    notifyListeners();
    try {
      await session?.api.setYolo(on);
    } catch (_) {}
  }

  Future<void> _open() async {
    final s = BlumiSession(conn!);
    await s.start();
    s.addListener(notifyListeners);
    session = s;
    connecting = false;
    status = 'connected';
    await _syncSavedServer();
    await refreshSessions();
    notifyListeners();
  }

  /// Persist the freshest token, and auto-label a still-host-named server (e.g.
  /// a migrated connection) with the gateway's machine hostname.
  Future<void> _syncSavedServer() async {
    final id = currentServerId;
    if (id == null) return;
    var cur = _byId(id);
    if (cur == null) return;
    if (conn?.token != null) cur = cur.copyWith(token: conn!.token);
    if (cur.name == cur.host) {
      try {
        final cfg = await ApiClient(conn!).config();
        final hn = (cfg['hostname'] as String?)?.trim();
        if (hn != null && hn.isNotEmpty) cur = cur.copyWith(name: hn);
      } catch (_) {}
    }
    servers = servers.map((sv) => sv.id == id ? cur! : sv).toList();
    await _saveServers();
  }

  // --- session ops -----------------------------------------------------------

  Future<void> refreshSessions() async {
    final s = session;
    if (s == null) return;
    try {
      sessions = await s.api.sessions();
      notifyListeners();
    } catch (_) {}
  }

  Future<void> newSession() async {
    final s = session;
    if (s == null) return;
    await s.api.newSession();
    await s.restore();
    await refreshSessions();
  }

  Future<void> resumeSession(String id) async {
    final s = session;
    if (s == null) return;
    await s.api.resume(id);
    await s.restore();
  }

  /// Close the live session and return to the connect screen (the server stays
  /// saved, so the user can reconnect or switch machines).
  Future<void> disconnect() async {
    session?.dispose();
    session = null;
    conn = null;
    currentServerId = null;
    status = '';
    notifyListeners();
  }

  // --- helpers ---------------------------------------------------------------

  SavedServer? _byId(String? id) {
    if (id == null) return null;
    for (final s in servers) {
      if (s.id == id) return s;
    }
    return null;
  }

  (String, int) _splitBase(String base) {
    var b = base.replaceFirst(RegExp(r'^https?://'), '');
    final slash = b.indexOf('/');
    if (slash >= 0) b = b.substring(0, slash);
    final colon = b.lastIndexOf(':');
    if (colon >= 0) {
      final port = int.tryParse(b.substring(colon + 1)) ?? 7777;
      return (b.substring(0, colon), port);
    }
    return (b, 7777);
  }
}
