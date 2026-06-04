import 'dart:async';
import 'package:flutter/material.dart';

/// The blumi "thinking" mascot — a faithful port of the TUI's animated flower:
/// a morphing petal glyph that color-sweeps along the Living Rose ramp
/// (rose → lavender → violet → cyan → mint), the word "thinking", and growing
/// `✿✿✿` petals. Shows the streamed reasoning text beneath, if any.

// Petal color anchors (RGB) for the swept gradient — mirrors mascot.rs.
const _anchors = <Color>[
  Color(0xFFFF4F87), // rose-pink
  Color(0xFF9B86FF), // lavender
  Color(0xFF6B50FF), // violet
  Color(0xFF68FFD6), // cyan
  Color(0xFF4FE0A0), // mint
];
const _steps = 6; // frames per ramp segment (smoothness)

// Morphing petal glyphs — the flower "blooms"/turns as the tick advances.
const _petals = ['✿', '❀', '❁', '✾', '❃', '❀', '✿', '❋'];

Color _ramp(int tick) {
  final total = _anchors.length * _steps;
  final idx = tick % total;
  final seg = idx ~/ _steps;
  final t = (idx % _steps) / _steps;
  return Color.lerp(_anchors[seg], _anchors[(seg + 1) % _anchors.length], t)!;
}

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

  @override
  void initState() {
    super.initState();
    // ~11fps — matches the TUI's gradient-spinner cadence. Repaints only this
    // leaf widget (no session notify), so the chat list isn't rebuilt.
    _timer = Timer.periodic(const Duration(milliseconds: 90), (_) {
      if (mounted) setState(() => _tick++);
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final glyph = _petals[_tick % _petals.length];
    final dots = ['', '✿', '✿✿', '✿✿✿'][(_tick ~/ 3) % 4];
    final detail = widget.detail?.trim() ?? '';
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(vertical: 6),
          child: Text.rich(
            TextSpan(children: [
              TextSpan(
                text: '$glyph ',
                style: TextStyle(
                    color: _ramp(_tick),
                    fontWeight: FontWeight.bold,
                    fontSize: 15),
              ),
              TextSpan(text: 'thinking', style: TextStyle(color: _ramp(_tick + 4))),
              TextSpan(text: dots, style: TextStyle(color: _ramp(_tick + 8))),
            ]),
          ),
        ),
        if (detail.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(left: 4, bottom: 6),
            child: Text(
              detail,
              style: TextStyle(
                  fontStyle: FontStyle.italic,
                  fontSize: 13,
                  color: cs.onSurface.withValues(alpha: 0.55)),
            ),
          ),
      ],
    );
  }
}
