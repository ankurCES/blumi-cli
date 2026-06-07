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
