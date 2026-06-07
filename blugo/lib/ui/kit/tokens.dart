import 'package:flutter/material.dart';

/// The **Living Rose** brand ramp (rose → lavender → violet → cyan → mint),
/// constant across every palette so it reads as blumi's identity. Mirrors the
/// TUI mascot + the web/wiki gradient.
const roseRamp = <Color>[
  Color(0xFFFF4F87), // rose-pink
  Color(0xFF9B86FF), // lavender
  Color(0xFF6B50FF), // violet
  Color(0xFF68FFD6), // cyan
  Color(0xFF4FE0A0), // mint
];

/// Theme-wide design tokens attached to [ThemeData] via a [ThemeExtension], so
/// every kit widget + screen reads one source of truth (no hardcoded palette)
/// and theme switches animate via [lerp]. Read with `BlumiTokens.of(context)`.
@immutable
class BlumiTokens extends ThemeExtension<BlumiTokens> {
  /// Brand ramp anchors (Living Rose) — kept constant across palettes.
  final List<Color> ramp;

  /// Convenience diagonal brand gradient built from [ramp].
  final Gradient brandGradient;

  // Status vocabulary.
  final Color success, warning, error, info;

  /// AA-safe dim text on `surface` (replaces ad-hoc low-alpha onSurface).
  final Color textMuted;

  // Geometry.
  final double radiusSm, radiusMd, radiusLg;
  final double space1, space2, space3, space4;

  const BlumiTokens({
    required this.ramp,
    required this.brandGradient,
    required this.success,
    required this.warning,
    required this.error,
    required this.info,
    required this.textMuted,
    this.radiusSm = 10,
    this.radiusMd = 16,
    this.radiusLg = 24,
    this.space1 = 4,
    this.space2 = 8,
    this.space3 = 12,
    this.space4 = 16,
  });

  /// Build the tokens for a palette. `ramp` stays the brand ramp; status +
  /// muted text are tuned per-palette for contrast.
  factory BlumiTokens.from({
    required Color success,
    required Color error,
    required Color textMuted,
    Color? warning,
    Color? info,
  }) {
    return BlumiTokens(
      ramp: roseRamp,
      brandGradient: const LinearGradient(
        begin: Alignment.topLeft,
        end: Alignment.bottomRight,
        colors: roseRamp,
      ),
      success: success,
      warning: warning ?? const Color(0xFFFFC857),
      error: error,
      info: info ?? const Color(0xFF68FFD6),
      textMuted: textMuted,
    );
  }

  /// Read the tokens, falling back to a rose default if none are attached.
  static BlumiTokens of(BuildContext context) =>
      Theme.of(context).extension<BlumiTokens>() ?? _fallback;

  static final BlumiTokens _fallback = BlumiTokens.from(
    success: const Color(0xFF4FE0A0),
    error: const Color(0xFFFF5470),
    textMuted: const Color(0xFFB89AA6),
  );

  @override
  BlumiTokens copyWith({
    List<Color>? ramp,
    Gradient? brandGradient,
    Color? success,
    Color? warning,
    Color? error,
    Color? info,
    Color? textMuted,
    double? radiusSm,
    double? radiusMd,
    double? radiusLg,
    double? space1,
    double? space2,
    double? space3,
    double? space4,
  }) {
    return BlumiTokens(
      ramp: ramp ?? this.ramp,
      brandGradient: brandGradient ?? this.brandGradient,
      success: success ?? this.success,
      warning: warning ?? this.warning,
      error: error ?? this.error,
      info: info ?? this.info,
      textMuted: textMuted ?? this.textMuted,
      radiusSm: radiusSm ?? this.radiusSm,
      radiusMd: radiusMd ?? this.radiusMd,
      radiusLg: radiusLg ?? this.radiusLg,
      space1: space1 ?? this.space1,
      space2: space2 ?? this.space2,
      space3: space3 ?? this.space3,
      space4: space4 ?? this.space4,
    );
  }

  @override
  BlumiTokens lerp(ThemeExtension<BlumiTokens>? other, double t) {
    if (other is! BlumiTokens) return this;
    return BlumiTokens(
      ramp: t < 0.5 ? ramp : other.ramp,
      brandGradient: t < 0.5 ? brandGradient : other.brandGradient,
      success: Color.lerp(success, other.success, t)!,
      warning: Color.lerp(warning, other.warning, t)!,
      error: Color.lerp(error, other.error, t)!,
      info: Color.lerp(info, other.info, t)!,
      textMuted: Color.lerp(textMuted, other.textMuted, t)!,
      radiusSm: _lerpD(radiusSm, other.radiusSm, t),
      radiusMd: _lerpD(radiusMd, other.radiusMd, t),
      radiusLg: _lerpD(radiusLg, other.radiusLg, t),
      space1: _lerpD(space1, other.space1, t),
      space2: _lerpD(space2, other.space2, t),
      space3: _lerpD(space3, other.space3, t),
      space4: _lerpD(space4, other.space4, t),
    );
  }

  static double _lerpD(double a, double b, double t) => a + (b - a) * t;
}
