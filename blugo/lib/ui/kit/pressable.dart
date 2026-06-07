import 'package:flutter/material.dart';
import 'motion.dart';

/// Tactile press feedback: the child scales down slightly while held. Wraps any
/// tappable surface (cards, node glyphs, palette rows) for a consistent feel.
/// No-op scale under reduced motion (still fires [onTap]).
class PressableScale extends StatefulWidget {
  final Widget child;
  final VoidCallback? onTap;
  final VoidCallback? onLongPress;
  final double pressedScale;
  final HitTestBehavior behavior;
  const PressableScale({
    required this.child,
    this.onTap,
    this.onLongPress,
    this.pressedScale = 0.96,
    this.behavior = HitTestBehavior.opaque,
    super.key,
  });

  @override
  State<PressableScale> createState() => _PressableScaleState();
}

class _PressableScaleState extends State<PressableScale> {
  bool _down = false;
  void _set(bool v) {
    if (widget.onTap == null && widget.onLongPress == null) return;
    if (_down != v) setState(() => _down = v);
  }

  @override
  Widget build(BuildContext context) {
    final enabled = widget.onTap != null || widget.onLongPress != null;
    final reduce = reducedMotion(context);
    return GestureDetector(
      behavior: widget.behavior,
      onTap: widget.onTap,
      onLongPress: widget.onLongPress,
      onTapDown: enabled ? (_) => _set(true) : null,
      onTapUp: enabled ? (_) => _set(false) : null,
      onTapCancel: enabled ? () => _set(false) : null,
      child: AnimatedScale(
        scale: (_down && !reduce) ? widget.pressedScale : 1.0,
        duration: Motion.fast,
        curve: Motion.curve,
        child: widget.child,
      ),
    );
  }
}
