import 'dart:math' as math;
import 'dart:typed_data' show Float64List;
import 'dart:ui' as ui;
import 'package:flutter/material.dart';
import 'tokens.dart';

/// Sample the Living-Rose ramp at a continuous position `t` in [0,1] (wraps),
/// blending between adjacent anchors. Used by the mascot, node gradients, and
/// any swept-gradient accent so the brand color motion is consistent.
Color rampAt(double t) {
  final n = roseRamp.length;
  final x = (t % 1.0) * n;
  final i = x.floor() % n;
  final f = x - x.floorToDouble();
  return Color.lerp(roseRamp[i], roseRamp[(i + 1) % n], f)!;
}

/// Discrete ramp sample by integer tick (mirrors the TUI mascot cadence):
/// `steps` frames per anchor segment.
Color rampTick(int tick, {int steps = 6}) {
  final total = roseRamp.length * steps;
  final idx = tick % total;
  final seg = idx ~/ steps;
  final f = (idx % steps) / steps;
  return Color.lerp(roseRamp[seg], roseRamp[(seg + 1) % roseRamp.length], f)!;
}

/// A text widget whose characters sweep through the brand ramp — for hero
/// wordmarks ("blumi"/"blugo"). Static (no animation) by default.
class GradientText extends StatelessWidget {
  final String text;
  final TextStyle? style;
  final Gradient? gradient;
  const GradientText(this.text, {this.style, this.gradient, super.key});

  @override
  Widget build(BuildContext context) {
    final g = gradient ?? BlumiTokens.of(context).brandGradient;
    return ShaderMask(
      blendMode: BlendMode.srcIn,
      shaderCallback: (rect) => g.createShader(rect),
      child: Text(text, style: style),
    );
  }
}

/// The blumi logo flower — an eight-petal bloom around a cyan nucleus, filled
/// with the Living-Rose gradient. A faithful port of `assets/blumi-logo.svg`
/// (4 axis + 4 diagonal petals, each diagonal rotated about its own centre).
/// This is *the* brand mark, so it keeps the logo's fixed colors regardless of
/// the active theme. Pass [dim] to render it faded (e.g. an un-added grid node).
class BloomFlower extends StatelessWidget {
  final double size;
  final bool dim;
  const BloomFlower({required this.size, this.dim = false, super.key});

  @override
  Widget build(BuildContext context) {
    final flower = SizedBox.square(
      dimension: size,
      child: CustomPaint(painter: _BloomFlowerPainter()),
    );
    return dim ? Opacity(opacity: 0.5, child: flower) : flower;
  }
}

class _BloomFlowerPainter extends CustomPainter {
  // [cx, cy, rx, ry, degrees] in the SVG's flower-local coordinates.
  static const _petals = <List<double>>[
    [0, -36, 19, 33, 0],
    [0, 36, 19, 33, 0],
    [-36, 0, 33, 19, 0],
    [36, 0, 33, 19, 0],
    [-26, -26, 28, 15, 45],
    [26, 26, 28, 15, 45],
    [26, -26, 28, 15, -45],
    [-26, 26, 28, 15, -45],
  ];

  @override
  void paint(Canvas canvas, Size size) {
    const ext = 70.0; // half-extent: the axis petals reach ±69
    final scale = size.width / (ext * 2);
    canvas.save();
    canvas.translate(size.width / 2, size.height / 2);
    canvas.scale(scale);

    // One combined path so the gradient sweeps the whole bloom coherently
    // (rather than rotating per petal).
    final flower = Path();
    for (final p in _petals) {
      final oval = Path()
        ..addOval(Rect.fromCenter(
            center: Offset(p[0], p[1]), width: p[2] * 2, height: p[3] * 2));
      if (p[4] == 0) {
        flower.addPath(oval, Offset.zero);
      } else {
        flower.addPath(
            oval.transform(_rotateAbout(p[0], p[1], p[4])), Offset.zero);
      }
    }

    const rect = Rect.fromLTRB(-ext, -ext, ext, ext);
    final paint = Paint()
      ..isAntiAlias = true
      ..shader = ui.Gradient.linear(
        rect.topLeft,
        rect.bottomRight,
        const [
          Color(0xFFFF4F87), // rose
          Color(0xFF9B86FF), // lavender
          Color(0xFF6B50FF), // violet
          Color(0xFF68FFD6), // cyan
        ],
        const [0.0, 0.45, 0.75, 1.0],
      );
    canvas.drawPath(flower, paint);

    // Nucleus ◉ — cyan disc with a dark eye.
    canvas.drawCircle(Offset.zero, 17, Paint()..color = const Color(0xFF68FFD6));
    canvas.drawCircle(Offset.zero, 7.5, Paint()..color = const Color(0xFF0E1116));
    canvas.restore();
  }

  @override
  bool shouldRepaint(covariant CustomPainter old) => false;
}

/// Column-major 4×4 (as `Path.transform` expects) for rotating [deg] degrees
/// about the pivot ([cx], [cy]) — i.e. translate(c) · rotate · translate(-c).
Float64List _rotateAbout(double cx, double cy, double deg) {
  final r = deg * math.pi / 180;
  final a = math.cos(r), b = math.sin(r);
  final tx = cx - a * cx + b * cy;
  final ty = cy - b * cx - a * cy;
  return Float64List.fromList(<double>[
    a, b, 0, 0, //
    -b, a, 0, 0, //
    0, 0, 1, 0, //
    tx, ty, 0, 1, //
  ]);
}
