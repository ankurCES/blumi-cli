import 'package:flutter/material.dart';
import 'tokens.dart';

/// Chrome for a modal bottom sheet: a drag handle, a title row, and a scrollable
/// body inset for the keyboard + safe area. The single entry point for sheets so
/// they all share shape, padding, and dismiss affordance.
Future<T?> showBlumiSheet<T>(
  BuildContext context, {
  required String title,
  required Widget child,
  IconData? icon,
  bool isScrollControlled = true,
}) {
  final cs = Theme.of(context).colorScheme;
  final t = BlumiTokens.of(context);
  return showModalBottomSheet<T>(
    context: context,
    isScrollControlled: isScrollControlled,
    backgroundColor: cs.surface,
    barrierColor: Colors.black.withValues(alpha: 0.55),
    shape: RoundedRectangleBorder(
      borderRadius: BorderRadius.vertical(top: Radius.circular(t.radiusLg)),
    ),
    builder: (context) => SheetScaffold(title: title, icon: icon, child: child),
  );
}

/// The inner layout used by [showBlumiSheet] (also reusable directly): grab
/// handle, title row with optional icon + close button, then the body.
class SheetScaffold extends StatelessWidget {
  final String title;
  final IconData? icon;
  final Widget child;
  const SheetScaffold(
      {required this.title, required this.child, this.icon, super.key});

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final t = BlumiTokens.of(context);
    final bottom = MediaQuery.of(context).viewInsets.bottom;
    return Padding(
      padding: EdgeInsets.only(bottom: bottom),
      child: SafeArea(
        top: false,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const SizedBox(height: 8),
            Container(
              width: 38,
              height: 4,
              decoration: BoxDecoration(
                color: cs.onSurface.withValues(alpha: 0.18),
                borderRadius: BorderRadius.circular(2),
              ),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(18, 14, 8, 6),
              child: Row(
                children: [
                  if (icon != null) ...[
                    Icon(icon, size: 18, color: t.ramp.first),
                    const SizedBox(width: 8),
                  ],
                  Expanded(
                    child: Text(
                      title,
                      style: const TextStyle(
                          fontSize: 17, fontWeight: FontWeight.w700),
                    ),
                  ),
                  IconButton(
                    icon: const Icon(Icons.close, size: 20),
                    color: t.textMuted,
                    onPressed: () => Navigator.of(context).maybePop(),
                  ),
                ],
              ),
            ),
            Flexible(
              child: SingleChildScrollView(
                padding: const EdgeInsets.fromLTRB(18, 0, 18, 18),
                child: child,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// A consistent confirm/deny dialog. Returns `true` on confirm. `danger` paints
/// the confirm action with the error color (for destructive ops like delete).
Future<bool> confirmDialog(
  BuildContext context, {
  required String title,
  required String message,
  String confirmLabel = 'Confirm',
  String cancelLabel = 'Cancel',
  bool danger = false,
  IconData? icon,
}) async {
  final t = BlumiTokens.of(context);
  final res = await showDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      icon: icon != null
          ? Icon(icon, color: danger ? t.error : t.ramp.first)
          : null,
      title: Text(title),
      content: Text(message, style: TextStyle(color: t.textMuted, height: 1.4)),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(false),
          child: Text(cancelLabel),
        ),
        FilledButton(
          style: danger
              ? FilledButton.styleFrom(backgroundColor: t.error)
              : null,
          onPressed: () => Navigator.of(context).pop(true),
          child: Text(confirmLabel),
        ),
      ],
    ),
  );
  return res ?? false;
}

/// A screen header: optional leading widget (e.g. Hero logo), a title + optional
/// subtitle, and trailing actions. Used by full-screen pages (connect/home).
class PageHeader extends StatelessWidget {
  final String title;
  final String? subtitle;
  final Widget? leading;
  final List<Widget> actions;
  const PageHeader({
    required this.title,
    this.subtitle,
    this.leading,
    this.actions = const [],
    super.key,
  });

  @override
  Widget build(BuildContext context) {
    final t = BlumiTokens.of(context);
    return Padding(
      padding: const EdgeInsets.fromLTRB(18, 14, 12, 8),
      child: Row(
        children: [
          if (leading != null) ...[leading!, const SizedBox(width: 12)],
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(title,
                    style: const TextStyle(
                        fontSize: 20, fontWeight: FontWeight.w800)),
                if (subtitle != null)
                  Text(subtitle!,
                      style: TextStyle(fontSize: 12.5, color: t.textMuted)),
              ],
            ),
          ),
          ...actions,
        ],
      ),
    );
  }
}
