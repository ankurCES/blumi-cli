import 'package:flutter/material.dart';
import '../data/models.dart';
import '../data/voice.dart';
import '../state/app.dart';
import 'control.dart';
import 'kit/kit.dart';

/// A quick-action command palette (opened from the composer's `/` or a header
/// action) mirroring the TUI slash palette.
Future<void> showCommandPalette(BuildContext context, AppController app) {
  return showBlumiSheet(
    context,
    title: 'Commands',
    icon: Icons.bolt,
    child: _Palette(app, host: context),
  );
}

class _PaletteCmd {
  final IconData icon;
  final String label;
  final Future<void> Function() run;
  const _PaletteCmd(this.icon, this.label, this.run);
}

class _Palette extends StatelessWidget {
  final AppController app;
  final BuildContext host; // stays mounted after the sheet pops
  const _Palette(this.app, {required this.host});

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    final t = BlumiTokens.of(context);
    final s = app.session;
    final busy = s?.busy ?? false;

    final cmds = <_PaletteCmd>[
      _PaletteCmd(Icons.add_comment_outlined, 'New session',
          () async => app.newSession()),
      if (busy)
        _PaletteCmd(Icons.stop_circle_outlined, 'Cancel current turn',
            () async => s?.cancel()),
      _PaletteCmd(
          Icons.compress, 'Compact context', () async => s?.api.compact()),
      _PaletteCmd(Icons.undo, 'Undo last change', () async => s?.api.undo()),
      _PaletteCmd(app.yolo ? Icons.flash_off : Icons.bolt,
          app.yolo ? 'Disable YOLO' : 'Enable YOLO',
          () async => app.setYolo(!app.yolo)),
      _PaletteCmd(Icons.tune, 'Control center',
          () async => showControlCenter(host, app)),
      _PaletteCmd(Icons.volume_up_outlined, 'Speak last reply', () async {
        String? text;
        for (final e in s?.entries.reversed ?? const <Entry>[]) {
          if (e is AssistantEntry) {
            text = e.text;
            break;
          }
        }
        if (text == null || text.trim().isEmpty) return;
        try {
          await voice.play(await s!.api.speak(text));
        } catch (_) {}
      }),
      _PaletteCmd(
          Icons.logout, 'Switch gateway', () async => app.disconnect()),
    ];

    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        for (final c in cmds)
          PressableScale(
            onTap: () {
              Navigator.of(context).pop();
              c.run();
            },
            child: Padding(
              padding: const EdgeInsets.symmetric(vertical: 11, horizontal: 2),
              child: Row(children: [
                Container(
                  width: 34,
                  height: 34,
                  decoration: BoxDecoration(
                    color: cs.secondary.withValues(alpha: 0.12),
                    borderRadius: BorderRadius.circular(t.radiusSm),
                  ),
                  child: Icon(c.icon, size: 18, color: cs.secondary),
                ),
                const SizedBox(width: 14),
                Expanded(
                    child: Text(c.label, style: const TextStyle(fontSize: 15))),
                Icon(Icons.chevron_right, size: 18, color: t.textMuted),
              ]),
            ),
          ),
      ],
    );
  }
}
