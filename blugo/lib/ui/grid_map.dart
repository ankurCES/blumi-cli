import 'dart:math' as math;
import 'package:flutter/material.dart';
import '../state/app.dart';
import 'grid_node.dart';
import 'kit/kit.dart';
import 'node_sheets.dart';

/// The welcome screen's interactive grid diagram: a radial hub-and-spoke map of
/// this device (hub) plus saved and auto-discovered blumi gateways. A canvas
/// layer draws the connective tissue (hub glow, gradient spokes, dashed
/// discovered edges, a radar sweep); tappable [GridNode]s float on top.
class GridMap extends StatefulWidget {
  final AppController app;
  const GridMap(this.app, {super.key});

  @override
  State<GridMap> createState() => _GridMapState();
}

class _GridMapState extends State<GridMap>
    with SingleTickerProviderStateMixin {
  late final AnimationController _ambient;

  @override
  void initState() {
    super.initState();
    _ambient = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 3600),
    );
    if (!reducedMotion(context)) _ambient.repeat();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    // Respect a runtime change to the reduce-motion setting.
    if (reducedMotion(context)) {
      _ambient.stop();
    } else if (!_ambient.isAnimating) {
      _ambient.repeat();
    }
  }

  @override
  void dispose() {
    _ambient.dispose();
    super.dispose();
  }

  void _tap(GridNodeVM vm) {
    switch (vm.kind) {
      case NodeKind.hub:
        showAddNodeSheet(context, widget.app);
        break;
      case NodeKind.saved:
        showSavedNodeSheet(context, widget.app, vm.server!);
        break;
      case NodeKind.discovered:
        showDiscoveredNodeSheet(context, widget.app, vm.server!);
        break;
    }
  }

  @override
  Widget build(BuildContext context) {
    final app = widget.app;
    final cs = Theme.of(context).colorScheme;
    final t = BlumiTokens.of(context);

    return ListenableBuilder(
      listenable: app,
      builder: (context, _) {
        final ring = <GridNodeVM>[
          for (final s in app.servers)
            GridNodeVM(
              kind: NodeKind.saved,
              server: s,
              label: s.name,
              sublabel: s.endpoint,
              isCurrent: app.currentServerId == s.id && app.connected,
              connecting: app.connecting && app.currentServerId == s.id,
            ),
          for (final s in app.discovered)
            GridNodeVM(
              kind: NodeKind.discovered,
              server: s,
              label: s.name,
              sublabel: s.endpoint,
            ),
        ];
        const hub = GridNodeVM(
          kind: NodeKind.hub,
          label: 'this device',
          sublabel: 'blugo',
        );

        return LayoutBuilder(
          builder: (context, c) {
            final w = c.maxWidth, h = c.maxHeight;
            final center = Offset(w / 2, h / 2);
            final count = ring.length;
            const hubD = 78.0;
            final nodeD = count > 6 ? 54.0 : 64.0;
            const labelPad = 36.0;
            final ringR = (math.min(w, h) / 2 - nodeD / 2 - labelPad - 6)
                .clamp(nodeD * 1.3, math.max(nodeD * 1.3, h));

            Offset posFor(int i) {
              final ang = -math.pi / 2 + 2 * math.pi * i / count;
              return center +
                  Offset(math.cos(ang), math.sin(ang)) * ringR.toDouble();
            }

            // Painter edges (hub → each ring node), colored along the ramp.
            final edges = <GridEdge>[
              for (var i = 0; i < count; i++)
                GridEdge(
                  target: posFor(i),
                  dashed: ring[i].kind == NodeKind.discovered,
                  color: ring[i].kind == NodeKind.discovered
                      ? t.textMuted
                      : rampAt(count <= 1 ? 0.0 : i / count),
                ),
            ];

            double labelWidth(double d) => math.max(d + 28, 104.0);

            Widget place(GridNodeVM vm, Offset p, double d) {
              final lw = labelWidth(d);
              return Positioned(
                left: p.dx - lw / 2,
                top: p.dy - d / 2,
                width: lw,
                child: GridNode(vm: vm, diameter: d, onTap: () => _tap(vm)),
              );
            }

            return Stack(
              clipBehavior: Clip.none,
              children: [
                // Connective tissue (only this layer repaints each frame).
                Positioned.fill(
                  child: RepaintBoundary(
                    child: AnimatedBuilder(
                      animation: _ambient,
                      builder: (context, _) => CustomPaint(
                        painter: _NetPainter(
                          center: center,
                          ringR: ringR.toDouble(),
                          hubR: hubD / 2,
                          edges: edges,
                          phase: _ambient.value,
                          scanning: !reducedMotion(context),
                          hubGlow: cs.primary,
                          accent: cs.secondary,
                          guide: cs.onSurface,
                        ),
                      ),
                    ),
                  ),
                ),

                // Ring nodes (staggered entrance; animate to new positions).
                for (var i = 0; i < count; i++)
                  Builder(builder: (context) {
                    final p = posFor(i);
                    final lw = labelWidth(nodeD);
                    return AnimatedPositioned(
                      duration: Motion.med,
                      curve: Motion.curve,
                      left: p.dx - lw / 2,
                      top: p.dy - nodeD / 2,
                      width: lw,
                      child: Entrance(
                        index: i,
                        child: GridNode(
                          vm: ring[i],
                          diameter: nodeD,
                          onTap: () => _tap(ring[i]),
                        ),
                      ),
                    );
                  }),

                // Hub (this device) at the center.
                place(hub, center, hubD),

                // Empty/scanning hint when nothing is saved or discovered yet.
                if (count == 0)
                  Positioned(
                    left: 0,
                    right: 0,
                    top: center.dy + hubD / 2 + 40,
                    child: Column(
                      children: [
                        Text('Looking for gateways on your network…',
                            style:
                                TextStyle(color: t.textMuted, fontSize: 13)),
                        const SizedBox(height: 4),
                        Text('Tap the flower to add one by address.',
                            style: TextStyle(
                                color: t.textMuted.withValues(alpha: 0.7),
                                fontSize: 12)),
                      ],
                    ),
                  ),
              ],
            );
          },
        );
      },
    );
  }
}

/// One hub→node spoke for the painter.
class GridEdge {
  final Offset target;
  final bool dashed;
  final Color color;
  const GridEdge(
      {required this.target, required this.dashed, required this.color});
}

class _NetPainter extends CustomPainter {
  final Offset center;
  final double ringR;
  final double hubR;
  final List<GridEdge> edges;
  final double phase; // 0..1 ambient
  final bool scanning;
  final Color hubGlow;
  final Color accent;
  final Color guide;

  _NetPainter({
    required this.center,
    required this.ringR,
    required this.hubR,
    required this.edges,
    required this.phase,
    required this.scanning,
    required this.hubGlow,
    required this.accent,
    required this.guide,
  });

  @override
  void paint(Canvas canvas, Size size) {
    // Hub glow.
    canvas.drawCircle(
      center,
      hubR * 2.8,
      Paint()
        ..shader = RadialGradient(
          colors: [
            hubGlow.withValues(alpha: 0.28),
            hubGlow.withValues(alpha: 0.0),
          ],
        ).createShader(Rect.fromCircle(center: center, radius: hubR * 2.8)),
    );

    // Faint guide ring through the nodes.
    if (edges.isNotEmpty) {
      _dashedCircle(
        canvas,
        center,
        ringR,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1
          ..color = guide.withValues(alpha: 0.12),
        dash: 5,
        gap: 9,
      );
    }

    // Spokes.
    for (final e in edges) {
      final p = Paint()
        ..style = PaintingStyle.stroke
        ..strokeCap = StrokeCap.round
        ..strokeWidth = e.dashed ? 1.4 : 1.9
        ..shader = LinearGradient(
          colors: [
            hubGlow.withValues(alpha: e.dashed ? 0.18 : 0.32),
            e.color.withValues(alpha: e.dashed ? 0.5 : 0.85),
          ],
        ).createShader(Rect.fromPoints(center, e.target));
      if (e.dashed) {
        _dashedLine(canvas, center, e.target, p, dash: 6, gap: 6);
      } else {
        canvas.drawLine(center, e.target, p);
      }
    }

    // Radar sweep — a soft rotating beam signalling live discovery.
    if (scanning) {
      final ang = -math.pi / 2 + phase * 2 * math.pi;
      final end = center + Offset(math.cos(ang), math.sin(ang)) * ringR;
      canvas.drawLine(
        center,
        end,
        Paint()
          ..strokeWidth = 2
          ..strokeCap = StrokeCap.round
          ..color = accent.withValues(alpha: 0.30)
          ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 2.5),
      );
      // A small expanding ping ring.
      final pingR = ringR * (0.25 + 0.75 * phase);
      canvas.drawCircle(
        center,
        pingR,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1.2
          ..color = accent.withValues(alpha: 0.18 * (1 - phase)),
      );
    }
  }

  void _dashedLine(Canvas canvas, Offset a, Offset b, Paint paint,
      {double dash = 6, double gap = 5}) {
    final total = (b - a).distance;
    final dir = (b - a) / total;
    var d = 0.0;
    while (d < total) {
      final start = a + dir * d;
      final end = a + dir * math.min(d + dash, total);
      canvas.drawLine(start, end, paint);
      d += dash + gap;
    }
  }

  void _dashedCircle(Canvas canvas, Offset c, double r, Paint paint,
      {double dash = 6, double gap = 8}) {
    final circumference = 2 * math.pi * r;
    final step = (dash + gap) / circumference * 2 * math.pi;
    final arc = dash / circumference * 2 * math.pi;
    var a = -math.pi / 2;
    final rect = Rect.fromCircle(center: c, radius: r);
    while (a < -math.pi / 2 + 2 * math.pi) {
      canvas.drawArc(rect, a, arc, false, paint);
      a += step;
    }
  }

  @override
  bool shouldRepaint(covariant _NetPainter old) =>
      old.phase != phase ||
      old.ringR != ringR ||
      old.edges.length != edges.length ||
      old.scanning != scanning;
}
