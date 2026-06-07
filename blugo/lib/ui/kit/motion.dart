import 'package:flutter/material.dart';

/// Shared motion language for blugo. Durations are short and consistent so the
/// app feels quick; every animated leaf wraps in a [RepaintBoundary] and honors
/// the OS "reduce motion" switch via [reducedMotion].
class Motion {
  static const fast = Duration(milliseconds: 140);
  static const med = Duration(milliseconds: 240);
  static const slow = Duration(milliseconds: 420);
  static const curve = Curves.easeOutCubic;
  static const emphasized = Curves.easeOutBack;
}

/// True when the user has asked the platform to minimize animation. Animated
/// widgets should fall back to an instant/!static presentation when this is set.
bool reducedMotion(BuildContext context) =>
    MediaQuery.maybeOf(context)?.disableAnimations ?? false;

/// A staggered entrance (fade + small rise) for list/grid children. `index`
/// offsets the start so items cascade in. No-op under reduced motion.
class Entrance extends StatelessWidget {
  final int index;
  final Widget child;
  final int stagger; // ms between items
  const Entrance({required this.child, this.index = 0, this.stagger = 40, super.key});

  @override
  Widget build(BuildContext context) {
    if (reducedMotion(context)) return child;
    final delay = (index * stagger).clamp(0, 600);
    return TweenAnimationBuilder<double>(
      tween: Tween(begin: 0, end: 1),
      duration: Motion.med + Duration(milliseconds: delay),
      curve: Interval(
        (delay / (Motion.med.inMilliseconds + 600)).clamp(0.0, 0.9),
        1.0,
        curve: Motion.curve,
      ),
      child: child,
      builder: (context, v, child) => Opacity(
        opacity: v,
        child: Transform.translate(offset: Offset(0, (1 - v) * 10), child: child),
      ),
    );
  }
}

/// A standard cross-fade+scale switcher for swapping content (e.g. a value that
/// refreshes). Collapses to an instant swap under reduced motion.
class FadeSwitcher extends StatelessWidget {
  final Widget child;
  final Duration duration;
  const FadeSwitcher({required this.child, this.duration = Motion.med, super.key});

  @override
  Widget build(BuildContext context) {
    return AnimatedSwitcher(
      duration: reducedMotion(context) ? Duration.zero : duration,
      switchInCurve: Motion.curve,
      switchOutCurve: Motion.curve,
      transitionBuilder: (c, a) => FadeTransition(
        opacity: a,
        child: ScaleTransition(scale: Tween(begin: 0.98, end: 1.0).animate(a), child: c),
      ),
      child: child,
    );
  }
}

/// A fade-through page route (used for connect→home and full-screen pushes).
Route<T> fadeThroughRoute<T>(Widget page) {
  return PageRouteBuilder<T>(
    transitionDuration: Motion.med,
    reverseTransitionDuration: Motion.fast,
    pageBuilder: (_, _, _) => page,
    transitionsBuilder: (context, anim, _, child) {
      if (reducedMotion(context)) return child;
      return FadeTransition(
        opacity: CurveTween(curve: Motion.curve).animate(anim),
        child: child,
      );
    },
  );
}

/// The shared Hero tag for the blumi logo (connect screen → home shell bloom).
const heroLogoTag = 'blumi-logo-hero';
