import 'dart:math' as math;
import 'package:flutter/material.dart';
import '../data/saved_server.dart';
import 'kit/kit.dart';

/// What a node represents on the grid map.
enum NodeKind { hub, saved, discovered }

/// View-model for one node in the grid diagram. Built from [SavedServer] data
/// plus presentation flags; the hub (this device) carries no server.
class GridNodeVM {
  final SavedServer? server;
  final NodeKind kind;
  final String label;
  final String sublabel;
  final bool isCurrent; // the currently-connected saved gateway
  final bool connecting; // this node is mid-connect

  const GridNodeVM({
    required this.kind,
    required this.label,
    required this.sublabel,
    this.server,
    this.isCurrent = false,
    this.connecting = false,
  });

  String get id => server?.id ?? '__hub__';
}

/// A small flower mark drawn on the canvas (five Living-Rose petals) — blugo's
/// identity used as the glyph inside each grid node.
class FlowerGlyph extends StatelessWidget {
  final double size;
  final bool dim;
  const FlowerGlyph({required this.size, this.dim = false, super.key});

  @override
  Widget build(BuildContext context) => SizedBox.square(
        dimension: size,
        child: CustomPaint(painter: _FlowerPainter(dim: dim)),
      );
}

class _FlowerPainter extends CustomPainter {
  final bool dim;
  _FlowerPainter({required this.dim});

  @override
  void paint(Canvas canvas, Size size) {
    final c = size.center(Offset.zero);
    final r = size.width;
    final petalW = r * 0.30;
    final petalH = r * 0.46;
    final dist = r * 0.16;
    for (var i = 0; i < roseRamp.length; i++) {
      final ang = -math.pi / 2 + i * 2 * math.pi / roseRamp.length;
      final paint = Paint()
        ..color = roseRamp[i].withValues(alpha: dim ? 0.55 : 0.95)
        ..style = PaintingStyle.fill;
      canvas.save();
      canvas.translate(c.dx, c.dy);
      canvas.rotate(ang);
      canvas.drawOval(
        Rect.fromCenter(
            center: Offset(0, -(dist + petalH / 2)),
            width: petalW,
            height: petalH),
        paint,
      );
      canvas.restore();
    }
    canvas.drawCircle(
      c,
      r * 0.11,
      Paint()
        ..color = dim
            ? roseRamp[2].withValues(alpha: 0.7)
            : Colors.white.withValues(alpha: 0.92),
    );
  }

  @override
  bool shouldRepaint(covariant _FlowerPainter old) => old.dim != dim;
}

/// A tappable grid node: a circular glyph (hub = phone, saved = solid flower +
/// status ring, discovered = dashed ring + "＋" badge) with a label beneath.
class GridNode extends StatelessWidget {
  final GridNodeVM vm;
  final double diameter;
  final VoidCallback onTap;
  const GridNode({
    required this.vm,
    required this.diameter,
    required this.onTap,
    super.key,
  });

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final t = BlumiTokens.of(context);

    final ringColor = vm.connecting
        ? t.info
        : vm.isCurrent
            ? t.success
            : (vm.kind == NodeKind.discovered
                ? t.textMuted
                : cs.primary.withValues(alpha: 0.8));

    Widget disc;
    switch (vm.kind) {
      case NodeKind.hub:
        // The hub wears the blugo app logo — the armed-hornet mascot bursting
        // from the blumi flower bloom — inside a brand-gradient rim. (The ring
        // nodes keep the plain flower glyph.)
        disc = Container(
          width: diameter,
          height: diameter,
          padding: const EdgeInsets.all(3),
          decoration: BoxDecoration(
            shape: BoxShape.circle,
            gradient: t.brandGradient,
            boxShadow: [
              BoxShadow(
                  color: cs.primary.withValues(alpha: 0.45),
                  blurRadius: 22,
                  spreadRadius: 1),
            ],
          ),
          child: ClipOval(
            child: SizedBox.expand(
              child: Image.asset('assets/icon/blugo_hub.png', fit: BoxFit.cover),
            ),
          ),
        );
        break;
      case NodeKind.saved:
        disc = Container(
          width: diameter,
          height: diameter,
          decoration: BoxDecoration(
            shape: BoxShape.circle,
            color: cs.surface,
            border: Border.all(color: ringColor, width: 2.2),
            boxShadow: [
              BoxShadow(
                  color: ringColor.withValues(alpha: 0.28), blurRadius: 12),
            ],
          ),
          child: Center(child: BloomFlower(size: diameter * 0.56)),
        );
        break;
      case NodeKind.discovered:
        disc = SizedBox(
          width: diameter,
          height: diameter,
          child: CustomPaint(
            painter: _DashedRingPainter(color: t.textMuted),
            child: Center(child: BloomFlower(size: diameter * 0.5, dim: true)),
          ),
        );
        break;
    }

    final node = Stack(
      clipBehavior: Clip.none,
      children: [
        disc,
        if (vm.kind == NodeKind.discovered)
          Positioned(
            right: -2,
            top: -2,
            child: Container(
              padding: const EdgeInsets.all(3),
              decoration: BoxDecoration(
                color: cs.primary,
                shape: BoxShape.circle,
                border: Border.all(color: cs.surface, width: 1.5),
              ),
              child: Icon(Icons.add, size: diameter * 0.18, color: Colors.black),
            ),
          ),
        if (vm.isCurrent)
          Positioned(
            right: -2,
            bottom: -2,
            child: Container(
              padding: const EdgeInsets.all(2),
              decoration: BoxDecoration(
                color: t.success,
                shape: BoxShape.circle,
                border: Border.all(color: cs.surface, width: 1.5),
              ),
              child:
                  Icon(Icons.check, size: diameter * 0.16, color: Colors.black),
            ),
          ),
      ],
    );

    final labelWidth = math.max(diameter + 28, 104.0);
    return SizedBox(
      width: labelWidth,
      child: Semantics(
        button: true,
        label: '${vm.label}, ${vm.kind.name} gateway, ${vm.sublabel}',
        child: PressableScale(
          onTap: onTap,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              node,
              const SizedBox(height: 6),
              Text(
                vm.label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                textAlign: TextAlign.center,
                style: TextStyle(
                  fontSize: 12.5,
                  fontWeight: FontWeight.w700,
                  color: cs.onSurface,
                ),
              ),
              Text(
                vm.sublabel,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                textAlign: TextAlign.center,
                style: TextStyle(fontSize: 10.5, color: t.textMuted),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Paints a dashed circular ring (the discovered-node visual), with a soft
/// translucent fill so it reads as "not yet added".
class _DashedRingPainter extends CustomPainter {
  final Color color;
  _DashedRingPainter({required this.color});

  @override
  void paint(Canvas canvas, Size size) {
    final c = size.center(Offset.zero);
    final radius = size.width / 2 - 1.4;

    canvas.drawCircle(
        c, radius, Paint()..color = color.withValues(alpha: 0.08));

    final stroke = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 2
      ..strokeCap = StrokeCap.round
      ..color = color.withValues(alpha: 0.75);

    const dash = 0.42; // radians of arc drawn
    const gap = 0.34; // radians skipped
    var a = -math.pi / 2;
    while (a < -math.pi / 2 + 2 * math.pi) {
      canvas.drawArc(
        Rect.fromCircle(center: c, radius: radius),
        a,
        dash,
        false,
        stroke,
      );
      a += dash + gap;
    }
  }

  @override
  bool shouldRepaint(covariant _DashedRingPainter old) => old.color != color;
}
