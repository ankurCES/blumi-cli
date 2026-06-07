import 'dart:async';
import 'package:flutter/material.dart';
import 'kit/kit.dart';

/// The blumi "thinking" mascot — a faithful port of the TUI's animated flower:
/// a morphing petal glyph that color-sweeps along the Living Rose ramp
/// (rose → lavender → violet → cyan → mint), the word "thinking", and growing
/// `✿✿✿` petals. Shows the streamed reasoning text beneath, if any. The ramp is
/// the shared brand ramp ([rampTick]); honors reduce-motion (static glyph).

// Morphing petal glyphs — the flower "blooms"/turns as the tick advances.
const _petals = ['✿', '❀', '❁', '✾', '❃', '❀', '✿', '❋'];

class ThinkingMascot extends StatefulWidget {
  /// The streamed reasoning text (optional) shown under the animation.
  final String? detail;
  const ThinkingMascot({this.detail, super.key});

  @override
  State<ThinkingMascot> createState() => _ThinkingMascotState();
}

class _ThinkingMascotState extends State<ThinkingMascot> {
  Timer? _timer;
  int _tick = 0;
  bool _started = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    // ~11fps — matches the TUI's gradient-spinner cadence. Repaints only this
    // leaf widget (no session notify), so the chat list isn't rebuilt. Skipped
    // entirely when the OS asks to reduce motion.
    if (!_started && !reducedMotion(context)) {
      _started = true;
      _timer = Timer.periodic(const Duration(milliseconds: 90), (_) {
        if (mounted) setState(() => _tick++);
      });
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final t = BlumiTokens.of(context);
    final glyph = _petals[_tick % _petals.length];
    final dots = ['', '✿', '✿✿', '✿✿✿'][(_tick ~/ 3) % 4];
    final detail = widget.detail?.trim() ?? '';
    return RepaintBoundary(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 6),
            child: Text.rich(
              TextSpan(children: [
                TextSpan(
                  text: '$glyph ',
                  style: TextStyle(
                      color: rampTick(_tick),
                      fontWeight: FontWeight.bold,
                      fontSize: 15),
                ),
                TextSpan(
                    text: 'thinking',
                    style: TextStyle(color: rampTick(_tick + 4))),
                TextSpan(
                    text: dots, style: TextStyle(color: rampTick(_tick + 8))),
              ]),
            ),
          ),
          if (detail.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(left: 4, bottom: 6),
              child: Text(
                detail,
                style: TextStyle(
                    fontStyle: FontStyle.italic, fontSize: 13, color: t.textMuted),
              ),
            ),
        ],
      ),
    );
  }
}
