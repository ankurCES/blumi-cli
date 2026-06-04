import 'dart:convert';
import 'package:http/http.dart' as http;

/// A selectable ElevenLabs voice — display name + its `voice_id`.
class VoiceOption {
  final String id;
  final String name;
  const VoiceOption({required this.id, required this.name});
}

/// Fetch the voices available to an ElevenLabs account, used to populate the
/// voice-ID dropdown once a key is entered. The call doubles as an auth check:
/// it throws on an invalid key or network failure.
Future<List<VoiceOption>> fetchElevenLabsVoices(String apiKey,
    {String? baseUrl}) async {
  final base = (baseUrl == null || baseUrl.trim().isEmpty)
      ? 'https://api.elevenlabs.io/v1'
      : baseUrl.trim().replaceAll(RegExp(r'/+$'), '');
  final res = await http.get(
    Uri.parse('$base/voices'),
    headers: {'xi-api-key': apiKey, 'accept': 'application/json'},
  ).timeout(const Duration(seconds: 15));
  if (res.statusCode == 401) throw 'invalid API key';
  if (res.statusCode != 200) throw 'HTTP ${res.statusCode}';
  return parseElevenLabsVoices(res.body);
}

/// Parse an ElevenLabs `GET /v1/voices` response body into selectable options,
/// sorted by name. Pure (no I/O) so it can be unit-tested.
List<VoiceOption> parseElevenLabsVoices(String body) {
  final json = jsonDecode(body) as Map<String, dynamic>;
  final list = (json['voices'] as List?) ?? const [];
  final out = <VoiceOption>[];
  for (final v in list) {
    final m = v as Map<String, dynamic>;
    final id = m['voice_id']?.toString() ?? '';
    if (id.isEmpty) continue;
    out.add(VoiceOption(id: id, name: m['name']?.toString() ?? id));
  }
  out.sort((a, b) => a.name.toLowerCase().compareTo(b.name.toLowerCase()));
  return out;
}
