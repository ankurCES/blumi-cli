import 'dart:io';
import 'dart:typed_data';
import 'package:audioplayers/audioplayers.dart';
import 'package:path_provider/path_provider.dart';
import 'package:record/record.dart';

/// Mic capture (for speech-to-text) + playback (for text-to-speech). A single
/// shared instance is enough for the whole app.
class VoiceService {
  final AudioRecorder _rec = AudioRecorder();
  final AudioPlayer _player = AudioPlayer();
  bool recording = false;

  Future<bool> hasMic() => _rec.hasPermission();

  /// Begin recording to a temp .m4a (AAC). Returns false if mic is denied.
  Future<bool> start() async {
    if (!await _rec.hasPermission()) return false;
    final dir = await getTemporaryDirectory();
    final path = '${dir.path}/blugo_rec.m4a';
    await _rec.start(const RecordConfig(encoder: AudioEncoder.aacLc), path: path);
    recording = true;
    return true;
  }

  /// Stop recording and return the captured bytes (m4a), or null.
  Future<Uint8List?> stop() async {
    final path = await _rec.stop();
    recording = false;
    if (path == null) return null;
    final f = File(path);
    return await f.exists() ? f.readAsBytes() : null;
  }

  Future<void> play(List<int> bytes) =>
      _player.play(BytesSource(Uint8List.fromList(bytes)));
}

/// Shared voice service instance.
final voice = VoiceService();
