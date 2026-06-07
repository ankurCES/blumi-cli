import 'package:flutter/material.dart';
import 'tokens.dart';

/// The standard surface container — rounded, subtle border, optional brand
/// gradient edge for emphasis. Replaces ad-hoc `Container(decoration: …)` cards.
class BlumiCard extends StatelessWidget {
  final Widget child;
  final EdgeInsetsGeometry padding;
  final VoidCallback? onTap;
  final bool gradientBorder;
  final Color? color;

  const BlumiCard({
    required this.child,
    this.padding = const EdgeInsets.all(14),
    this.onTap,
    this.gradientBorder = false,
    this.color,
    super.key,
  });

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final t = BlumiTokens.of(context);
    final radius = BorderRadius.circular(t.radiusMd);
    final surface = color ?? cs.surface;

    Widget body = Material(
      color: surface,
      borderRadius: radius,
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: onTap,
        child: Padding(padding: padding, child: child),
      ),
    );

    if (gradientBorder) {
      return Container(
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(t.radiusMd + 1.5),
          gradient: t.brandGradient,
        ),
        padding: const EdgeInsets.all(1.5),
        child: body,
      );
    }
    return Container(
      decoration: BoxDecoration(
        borderRadius: radius,
        border: Border.all(color: cs.onSurface.withValues(alpha: 0.07)),
      ),
      child: body,
    );
  }
}

/// A small uppercase section label with an optional trailing action — replaces
/// the dozens of ad-hoc `_label()` headers.
class SectionHeader extends StatelessWidget {
  final String title;
  final IconData? icon;
  final Widget? trailing;
  const SectionHeader(this.title, {this.icon, this.trailing, super.key});

  @override
  Widget build(BuildContext context) {
    final t = BlumiTokens.of(context);
    return Padding(
      padding: const EdgeInsets.fromLTRB(2, 14, 2, 8),
      child: Row(
        children: [
          if (icon != null) ...[
            Icon(icon, size: 14, color: t.textMuted),
            const SizedBox(width: 6),
          ],
          Expanded(
            child: Text(
              title.toUpperCase(),
              style: TextStyle(
                color: t.textMuted,
                fontSize: 11,
                fontWeight: FontWeight.w700,
                letterSpacing: 1.1,
              ),
            ),
          ),
          ?trailing,
        ],
      ),
    );
  }
}

/// A friendly empty/zero-state — icon, line, optional CTA. Replaces the many
/// inconsistent "(none)" / "nothing here" placeholders.
class EmptyState extends StatelessWidget {
  final IconData icon;
  final String message;
  final String? hint;
  final Widget? action;
  const EmptyState({
    required this.icon,
    required this.message,
    this.hint,
    this.action,
    super.key,
  });

  @override
  Widget build(BuildContext context) {
    final t = BlumiTokens.of(context);
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 34, color: t.textMuted.withValues(alpha: 0.7)),
            const SizedBox(height: 12),
            Text(
              message,
              textAlign: TextAlign.center,
              style: TextStyle(color: t.textMuted, fontSize: 14),
            ),
            if (hint != null) ...[
              const SizedBox(height: 4),
              Text(
                hint!,
                textAlign: TextAlign.center,
                style: TextStyle(
                    color: t.textMuted.withValues(alpha: 0.7), fontSize: 12),
              ),
            ],
            if (action != null) ...[const SizedBox(height: 16), action!],
          ],
        ),
      ),
    );
  }
}

/// A monospace block for logs / diffs / JSON dumps, on a sunken surface.
class MonoBlock extends StatelessWidget {
  final String text;
  final int? maxLines;
  const MonoBlock(this.text, {this.maxLines, super.key});

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final t = BlumiTokens.of(context);
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(10),
      decoration: BoxDecoration(
        color: cs.surface.withValues(alpha: 0.5),
        borderRadius: BorderRadius.circular(t.radiusSm),
        border: Border.all(color: cs.onSurface.withValues(alpha: 0.06)),
      ),
      child: SelectableText(
        text,
        maxLines: maxLines,
        style: TextStyle(
          fontFamily: 'monospace',
          fontSize: 12.5,
          height: 1.4,
          color: cs.onSurface.withValues(alpha: 0.85),
        ),
      ),
    );
  }
}
