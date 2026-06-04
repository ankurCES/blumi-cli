import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:blugo/data/cache.dart';
import 'package:blugo/data/elevenlabs.dart';
import 'package:blugo/data/events.dart';
import 'package:blugo/data/saved_server.dart';
import 'package:blugo/state/app.dart';
import 'package:blugo/ui/connect.dart';

void main() {
  group('protocol parsing', () {
    test('token + tool_result', () {
      final tok = BlumiEvent.parse('{"type":"token","text":"hi"}');
      expect(tok, isA<TokenEvent>());
      expect((tok as TokenEvent).text, 'hi');

      final tr = BlumiEvent.fromMap({
        'type': 'tool_result',
        'id': 'c1',
        'name': 'Bash',
        'ok': true,
        'preview': 'done',
      });
      expect(tr, isA<ToolResultEvent>());
      expect((tr as ToolResultEvent).ok, isTrue);
    });

    test('approval + todo + done', () {
      final ap = BlumiEvent.fromMap({
        'type': 'approval_request',
        'request_id': 'r1',
        'tool': 'Bash',
        'summary': 'rm -rf',
        'dangerous': true,
      });
      expect((ap as ApprovalRequest).dangerous, isTrue);

      final td = BlumiEvent.fromMap({
        'type': 'todo_update',
        'items': [
          {'id': '1', 'content': 'x', 'status': 'in_progress'}
        ],
      });
      expect((td as TodoUpdate).items.first.status, TodoStatus.inProgress);

      expect(BlumiEvent.fromMap({'type': 'done', 'reason': 'completed'}),
          isA<DoneEvent>());
    });

    test('unknown + malformed are tolerated', () {
      expect(BlumiEvent.fromMap({'type': 'future_thing'}), isA<UnknownEvent>());
      expect(BlumiEvent.parse('not json'), isNull);
    });
  });

  group('SavedServer', () {
    test('round-trips JSON and derives id/base/endpoint', () {
      final s = SavedServer.create(
          name: 'Mac', host: '10.0.0.61', port: 7777, token: 'tok');
      expect(s.id, '10.0.0.61:7777');
      expect(s.base, 'http://10.0.0.61:7777');
      expect(s.endpoint, '10.0.0.61:7777');
      final back = SavedServer.fromJson(s.toJson());
      expect(back.name, 'Mac');
      expect(back.host, '10.0.0.61');
      expect(back.port, 7777);
      expect(back.token, 'tok');
    });

    test('accepts a full URL host and tolerates missing fields', () {
      final url =
          SavedServer.create(name: 'remote', host: 'https://x.example', port: 443);
      expect(url.base, 'https://x.example');

      final sparse = SavedServer.fromJson({'host': '1.2.3.4'});
      expect(sparse.port, 7777);
      expect(sparse.name, '1.2.3.4');
      expect(sparse.id, '1.2.3.4:7777');
      expect(sparse.token, isNull);
    });
  });

  group('DataCache', () {
    test('peek/put/isFresh (stale-while-revalidate primitives)', () {
      final c = DataCache();
      expect(c.peek('k'), isNull);
      expect(c.isFresh('k', const Duration(seconds: 5)), isFalse);
      c.put('k', {'a': 1});
      expect((c.peek('k') as Map)['a'], 1);
      expect(c.isFresh('k', const Duration(seconds: 5)), isTrue);
      expect(c.isFresh('missing', const Duration(seconds: 5)), isFalse);
      c.clear(); // cancels the debounced save timer
      expect(c.peek('k'), isNull);
    });
  });

  group('ElevenLabs voices', () {
    test('parses voice_id/name, drops empties, sorts by name', () {
      final voices = parseElevenLabsVoices('''
        {"voices":[
          {"voice_id":"v2","name":"Rachel"},
          {"voice_id":"v1","name":"Adam"},
          {"voice_id":"","name":"Bad"},
          {"name":"NoId"},
          {"voice_id":"v3"}
        ]}''');
      // 3 valid (v2, v1, v3); the empty-id and id-less ones are dropped.
      expect(voices.length, 3);
      // sorted case-insensitively by name: Adam, Rachel, then v3 (name == id)
      expect(voices[0].id, 'v1');
      expect(voices[0].name, 'Adam');
      expect(voices[1].name, 'Rachel');
      // missing name falls back to the id
      expect(voices[2].id, 'v3');
      expect(voices[2].name, 'v3');
    });

    test('tolerates an empty/absent voices list', () {
      expect(parseElevenLabsVoices('{"voices":[]}'), isEmpty);
      expect(parseElevenLabsVoices('{}'), isEmpty);
    });
  });

  testWidgets('connect screen renders the add form when no servers are saved',
      (tester) async {
    await tester.pumpWidget(MaterialApp(home: ConnectScreen(AppController())));
    expect(find.text('blugo'), findsOneWidget);
    expect(find.text('Connect'), findsOneWidget);
    expect(find.text('Host (Mac LAN IP)'), findsOneWidget);
  });
}
