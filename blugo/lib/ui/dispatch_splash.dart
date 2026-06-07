import 'dart:ui' show lerpDouble;
import 'package:flutter/material.dart';
import '../state/app.dart';
import 'dispatch_inbox.dart';
import 'grid_node.dart' show FlowerGlyph;
import 'kit/kit.dart';

/// An on-demand "splash" played when Dispatch is opened from the centre flower
/// of the welcome grid. On the current theme background, the blumi flower spins
/// while a brand glow blooms out from behind it and washes the whole screen in
/// the Living-Rose gradient — then it hands off to the Dispatch inbox. Tap to
/// skip. (Reduced-motion users bypass this entirely; see `GridMap`.)
class DispatchSplashScreen extends StatefulWidget {
  final AppController app;
  const DispatchSplashScreen(this.app, {super.key});

  @override
  State<DispatchSplashScreen> createState() => _DispatchSplashScreenState();
}

class _DispatchSplashScreenState extends State<DispatchSplashScreen>
    with TickerProviderStateMixin {
  late final AnimationController _intro; // one-shot: glow + wash + text
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
      duration: const Duration(milliseconds: 2000),
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
            final bloomT = _seg(0.08, 0.82, Curves.easeOutCubic);
            final bloomOp = _seg(0.08, 0.42);
            final washOp = _seg(0.40, 1.0) * 0.92;
            final contentOp = _seg(0.0, 0.22);
            final textOp = _seg(0.30, 0.62);
            final textDy = (1 - textOp) * 14;

            return Stack(
              fit: StackFit.expand,
              children: [
                // 1) The Living-Rose gradient washes the whole background in.
                Opacity(
                  opacity: washOp,
                  child: DecoratedBox(
                    decoration: BoxDecoration(gradient: t.brandGradient),
                  ),
                ),
                // 2) A brand glow blooming outward from behind the flower.
                DecoratedBox(
                  decoration: BoxDecoration(
                    gradient: RadialGradient(
                      radius: lerpDouble(0.04, 1.5, bloomT)!,
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
                // 3) The spinning flower + the "bluuuum mode" line.
                Center(
                  child: Opacity(
                    opacity: contentOp,
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        RotationTransition(
                          turns: _spin,
                          child: const FlowerGlyph(size: 124),
                        ),
                        const SizedBox(height: 30),
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
/// rest in shadowed white so it reads over both the dark bg and the gradient.
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
        // Shadow-only copy behind the gradient word (ShaderMask would otherwise
        // tint the shadow), then the gradient fill on top.
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
