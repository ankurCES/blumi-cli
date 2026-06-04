import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';
import '../data/api.dart';
import '../data/models.dart';
import 'session.dart';

/// Top-level controller: owns the connection + the live [BlumiSession], handles
/// login/persistence, and the sessions list.
class AppController extends ChangeNotifier {
  BlumiSession? session;
  ServerConn? conn;
  String status = '';
  bool connecting = false;
  List<SessionInfo> sessions = [];

  bool get connected => session != null;

  /// Reconnect from saved credentials on launch (returns false if none).
  Future<bool> tryAutoConnect() async {
    final p = await SharedPreferences.getInstance();
    final base = p.getString('baseUrl');
    if (base == null) return false;
    conn = ServerConn(base, p.getString('token'));
    await _open();
    return true;
  }

  Future<void> connect({
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
      conn = ServerConn(base, token);
      final p = await SharedPreferences.getInstance();
      await p.setString('baseUrl', base);
      if (token != null) {
        await p.setString('token', token);
      } else {
        await p.remove('token');
      }
      await _open();
    } catch (e) {
      status = '$e';
      connecting = false;
      notifyListeners();
    }
  }

  Future<void> _open() async {
    final s = BlumiSession(conn!);
    await s.start();
    s.addListener(notifyListeners);
    session = s;
    connecting = false;
    status = 'connected';
    await refreshSessions();
    notifyListeners();
  }

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

  Future<void> disconnect() async {
    session?.dispose();
    session = null;
    conn = null;
    status = '';
    final p = await SharedPreferences.getInstance();
    await p.remove('baseUrl');
    await p.remove('token');
    notifyListeners();
  }
}
