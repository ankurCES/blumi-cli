import 'dart:math' as math;
import 'dart:typed_data' show Float64List;
import 'dart:ui' as ui;
import 'package:flutter/material.dart';
import '../state/app.dart';
import 'dispatch_inbox.dart';
import 'kit/kit.dart';

/// An on-demand "splash" played when Dispatch is opened from the centre flower
/// of the welcome grid. The blumi flower (the eight-petal logo bloom) spins on
/// the current theme background while a brand glow blooms out from behind it and
/// washes the screen in the Living-Rose gradient — then the gradient **recedes
/// back to the dark background** and "Entering bluuuum mode…" fades in, before
/// handing off to the Dispatch inbox. Tap to skip; reduced-motion users bypass
/// it entirely (see `GridMap`).
class DispatchSplashScreen extends StatefulWidget {
  final AppController app;
  const DispatchSplashScreen(this.app, {super.key});

  @override
  State<DispatchSplashScreen> createState() => _DispatchSplashScreenState();
}

class _DispatchSplashScreenState extends State<DispatchSplashScreen>
    with TickerProviderStateMixin {
  late final AnimationController _intro; // one-shot: bloom → wash → dark → text
  late final AnimationController _spin; // continuous flower rotation
  bool _navigated = false;

  @override
  void initState() {
    super.initState();
    _spin = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 1100),
    )..repeat();
    _intro = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 2800),
    )..addStatusListener((s) {
        if (s == AnimationStatus.completed) _go();
      });
    _intro.forward();
  }

  @override
  void dispose() {
    _intro.dispose();
    _spin.dispose();
    super.dispose();
  }

  /// Enter Dispatch (once), cross-fading in. Safe to call from both the
  /// animation's completion and a tap-to-skip.
  void _go() {
    if (_navigated || !mounted) return;
    _navigated = true;
    Navigator.of(context).pushReplacement(PageRouteBuilder(
      transitionDuration: const Duration(milliseconds: 320),
      pageBuilder: (_, _, _) => DispatchInboxScreen(widget.app),
      transitionsBuilder: (_, anim, _, child) =>
          FadeTransition(opacity: anim, child: child),
    ));
  }

  /// Eased sub-progress of [_intro] over the window [a, b].
  double _seg(double a, double b, [Curve c = Curves.easeInOut]) {
    final x = ((_intro.value - a) / (b - a)).clamp(0.0, 1.0).toDouble();
    return c.transform(x);
  }

  @override
  Widget build(BuildContext context) {
    final t = BlumiTokens.of(context);
    final bg = Theme.of(context).scaffoldBackgroundColor;

    return Scaffold(
      backgroundColor: bg,
      body: GestureDetector(
        onTap: _go, // tap anywhere to skip
        behavior: HitTestBehavior.opaque,
        child: AnimatedBuilder(
          animation: _intro,
          builder: (context, _) {
            // The gradient rises then recedes, so the screen ends dark again.
            final rise = _seg(0.06, 0.38);
            final fall = _seg(0.44, 0.66);
            final swell = rise * (1 - fall); // 0 → peak → 0
            final bloomT = _seg(0.03, 0.45, Curves.easeOutCubic);
            final washOp = swell * 0.95;
            final bloomOp = swell;
            final contentOp = _seg(0.0, 0.12);
            final textOp = _seg(0.70, 0.86);
            final textDy = (1 - textOp) * 14;

            return Stack(
              fit: StackFit.expand,
              children: [
                // 1) The Living-Rose gradient washes in, then back out to dark.
                Opacity(
                  opacity: washOp,
                  child: DecoratedBox(
                    decoration: BoxDecoration(gradient: t.brandGradient),
                  ),
                ),
                // 2) A brand glow blooming outward from behind the flower
                //    (fades with the wash so the end state is dark).
                DecoratedBox(
                  decoration: BoxDecoration(
                    gradient: RadialGradient(
                      radius: ui.lerpDouble(0.04, 1.5, bloomT)!,
                      colors: [
                        Color.lerp(roseRamp[3], Colors.white, 0.35)!
                            .withValues(alpha: 0.85 * bloomOp),
                        roseRamp[2].withValues(alpha: 0.45 * bloomOp),
                        Colors.transparent,
                      ],
                      stops: const [0.0, 0.5, 1.0],
                    ),
                  ),
                ),
                // 3) The spinning blumi flower + the "bluuuum mode" line.
                Center(
                  child: Opacity(
                    opacity: contentOp,
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        RotationTransition(
                          turns: _spin,
                          child: const _BloomFlower(150),
                        ),
                        const SizedBox(height: 34),
                        Opacity(
                          opacity: textOp,
                          child: Transform.translate(
                            offset: Offset(0, textDy),
                            child: const _BluumLine(),
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ],
            );
          },
        ),
      ),
    );
  }
}

/// "Entering bluuuum mode…" — the brand word swept in the Living-Rose ramp, the
/// rest in shadowed white so it reads on the dark end background.
class _BluumLine extends StatelessWidget {
  const _BluumLine();

  @override
  Widget build(BuildContext context) {
    const base = TextStyle(
      fontSize: 19,
      fontWeight: FontWeight.w700,
      color: Colors.white,
      letterSpacing: 0.2,
      shadows: [Shadow(blurRadius: 10, color: Colors.black54)],
    );
    final bold = base.copyWith(fontWeight: FontWeight.w900);
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        const Text('Entering ', style: base),
        // Shadow-only copy behind the gradient word (the ShaderMask would
        // otherwise tint the shadow), then the gradient fill on top.
        Stack(
          children: [
            Text('bluuuum', style: bold.copyWith(color: Colors.transparent)),
            GradientText('bluuuum', style: bold.copyWith(shadows: const [])),
          ],
        ),
        const Text(' mode…', style: base),
      ],
    );
  }
}

/// The blumi logo flower — an eight-petal bloom around a cyan nucleus, filled
/// with the Living-Rose gradient. A faithful port of `assets/blumi-logo.svg`
/// (4 axis + 4 diagonal petals, each diagonal rotated about its own centre).
class _BloomFlower extends StatelessWidget {
  final double size;
  const _BloomFlower(this.size);

  @override
  Widget build(BuildContext context) => SizedBox.square(
        dimension: size,
        child: CustomPaint(painter: _BloomFlowerPainter()),
      );
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
