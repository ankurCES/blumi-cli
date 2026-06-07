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
                          child: const BloomFlower(size: 150),
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
