import 'package:flutter/material.dart';
import 'tokens.dart';

/// Semantic status for the one shared status vocabulary (dots, pills).
enum BlumiStatus { ok, warn, err, idle, busy, info }

extension BlumiStatusColor on BlumiStatus {
  Color color(BuildContext c) {
    final t = BlumiTokens.of(c);
    switch (this) {
      case BlumiStatus.ok:
        return t.success;
      case BlumiStatus.warn:
        return t.warning;
      case BlumiStatus.err:
        return t.error;
      case BlumiStatus.busy:
      case BlumiStatus.info:
        return t.info;
      case BlumiStatus.idle:
        return t.textMuted;
    }
  }
}

/// A small filled status dot (online/offline/etc.).
class StatusDot extends StatelessWidget {
  final BlumiStatus status;
  final double size;
  const StatusDot(this.status, {this.size = 9, super.key});

  @override
  Widget build(BuildContext context) {
    final c = status.color(context);
    return Container(
      width: size,
      height: size,
      decoration: BoxDecoration(
        color: c,
        shape: BoxShape.circle,
        boxShadow: [BoxShadow(color: c.withValues(alpha: 0.5), blurRadius: 5)],
      ),
    );
  }
}

/// A status dot + label pill.
class StatusPill extends StatelessWidget {
  final BlumiStatus status;
  final String label;
  const StatusPill(this.status, this.label, {super.key});

  @override
  Widget build(BuildContext context) {
    final c = status.color(context);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
      decoration: BoxDecoration(
        color: c.withValues(alpha: 0.13),
        borderRadius: BorderRadius.circular(20),
      ),
      child: Row(mainAxisSize: MainAxisSize.min, children: [
        StatusDot(status, size: 7),
        const SizedBox(width: 5),
        Text(label,
            style: TextStyle(color: c, fontSize: 11, fontWeight: FontWeight.w600)),
      ]),
    );
  }
}

/// A compact labelled chip/badge (e.g. an accelerator or version tag).
class BlumiBadge extends StatelessWidget {
  final String text;
  final IconData? icon;
  final Color? color;
  const BlumiBadge(this.text, {this.icon, this.color, super.key});

  @override
  Widget build(BuildContext context) {
    final t = BlumiTokens.of(context);
    final c = color ?? t.info;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
      decoration: BoxDecoration(
        color: c.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(t.radiusSm),
        border: Border.all(color: c.withValues(alpha: 0.3)),
      ),
      child: Row(mainAxisSize: MainAxisSize.min, children: [
        if (icon != null) ...[Icon(icon, size: 12, color: c), const SizedBox(width: 4)],
        Text(text,
            style: TextStyle(color: c, fontSize: 11, fontWeight: FontWeight.w600)),
      ]),
    );
  }
}

/// The primary call-to-action: a brand-gradient filled button with a busy state.
class GradientButton extends StatelessWidget {
  final String label;
  final IconData? icon;
  final VoidCallback? onPressed;
  final bool busy;
  final bool expand;
  const GradientButton({
    required this.label,
    this.icon,
    this.onPressed,
    this.busy = false,
    this.expand = true,
    super.key,
  });

  @override
  Widget build(BuildContext context) {
    final t = BlumiTokens.of(context);
    final enabled = onPressed != null && !busy;
    final radius = BorderRadius.circular(t.radiusSm + 2);
    final child = Row(
      mainAxisSize: expand ? MainAxisSize.max : MainAxisSize.min,
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        if (busy)
          const SizedBox(
              width: 16,
              height: 16,
              child: CircularProgressIndicator(
                  strokeWidth: 2, color: Colors.black))
        else if (icon != null)
          Icon(icon, size: 18, color: Colors.black),
        if (busy || icon != null) const SizedBox(width: 8),
        Text(label,
            style: const TextStyle(
                color: Colors.black, fontWeight: FontWeight.w700, fontSize: 15)),
      ],
    );
    return Opacity(
      opacity: enabled ? 1 : 0.5,
      child: Material(
        color: Colors.transparent,
        borderRadius: radius,
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: enabled ? onPressed : null,
          child: Ink(
            decoration: BoxDecoration(gradient: t.brandGradient, borderRadius: radius),
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 13),
              child: child,
            ),
          ),
        ),
      ),
    );
  }
}

/// A small inline spinner sized for rows/headers.
class InlineSpinner extends StatelessWidget {
  final double size;
  const InlineSpinner({this.size = 16, super.key});
  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: size,
      height: size,
      child: CircularProgressIndicator(
          strokeWidth: 2, color: Theme.of(context).colorScheme.primary),
    );
  }
}
