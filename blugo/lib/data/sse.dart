import 'dart:async';
import 'dart:convert';
import 'package:http/http.dart' as http;
import 'api.dart';
import 'events.dart';

/// Subscribes to the gateway's SSE stream (`GET /api/chat/stream`) and yields
/// parsed [BlumiEvent]s, tracking the last seq so a reconnect replays via
/// `Last-Event-ID` (the server's ring-buffer healing). Native HTTP lets us set
/// the `Authorization` header on the streaming request (unlike browser
/// EventSource), so Bearer auth works directly.
class EventStream {
  final ServerConn conn;
  final http.Client _http;
  int _lastSeq = 0;
  bool _closed = false;
  StreamController<BlumiEvent>? _ctrl;

  EventStream(this.conn, [http.Client? client])
      : _http = client ?? http.Client();

  Stream<BlumiEvent> connect() {
    _ctrl = StreamController<BlumiEvent>(onCancel: close);
    _run();
    return _ctrl!.stream;
  }

  Future<void> _run() async {
    while (!_closed) {
      try {
        final req =
            http.Request('GET', Uri.parse('${conn.baseUrl}/api/chat/stream'));
        req.headers['Accept'] = 'text/event-stream';
        // Only send Last-Event-ID on a reconnect (once we've seen events) so the
        // first connect is live-only — the transcript already came from
        // /api/messages, and replaying history here would duplicate it.
        if (_lastSeq > 0) {
          req.headers['Last-Event-ID'] = '$_lastSeq';
        }
        if (conn.token != null) {
          req.headers['Authorization'] = 'Bearer ${conn.token}';
        }
        final resp = await _http.send(req);
        if (resp.statusCode != 200) {
          await Future.delayed(const Duration(seconds: 2));
          continue;
        }
        var buf = '';
        await for (final chunk in resp.stream.transform(utf8.decoder)) {
          if (_closed) break;
          buf += chunk;
          int idx;
          while ((idx = buf.indexOf('\n\n')) >= 0) {
            _emit(buf.substring(0, idx));
            buf = buf.substring(idx + 2);
          }
        }
      } catch (_) {
        // fall through to the reconnect backoff
      }
      if (_closed) break;
      await Future.delayed(const Duration(seconds: 2));
    }
  }

  void _emit(String frame) {
    String? data;
    for (final raw in frame.split('\n')) {
      final line = raw.trimRight();
      if (line.startsWith('id:')) {
        final v = int.tryParse(line.substring(3).trim());
        if (v != null) _lastSeq = v;
      } else if (line.startsWith('data:')) {
        data = line.substring(5).trim();
      }
    }
    if (data != null && data.isNotEmpty) {
      final ev = BlumiEvent.parse(data);
      if (ev != null) _ctrl?.add(ev);
    }
  }

  void close() {
    _closed = true;
    _ctrl?.close();
  }
}
