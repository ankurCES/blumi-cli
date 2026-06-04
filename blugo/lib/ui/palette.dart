import 'package:flutter/material.dart';
import '../data/models.dart';
import '../data/voice.dart';
import '../state/app.dart';
import 'control.dart';

/// A quick-action command palette (opened from the composer's `/` or a header
/// action) mirroring the TUI slash palette.
Future<void> showCommandPalette(BuildContext context, AppController app) {
  return showModalBottomSheet(
    context: context,
    showDragHandle: true,
    backgroundColor: Theme.of(context).colorScheme.surface,
    builder: (_) => _Palette(app, host: context),
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
    final s = app.session;
    final busy = s?.busy ?? false;

    final cmds = <_PaletteCmd>[
      _PaletteCmd(Icons.add_comment_outlined, 'New session', () async => app.newSession()),
      if (busy)
        _PaletteCmd(Icons.stop_circle_outlined, 'Cancel current turn',
            () async => s?.cancel()),
      _PaletteCmd(Icons.compress, 'Compact context', () async => s?.api.compact()),
      _PaletteCmd(Icons.undo, 'Undo last change', () async => s?.api.undo()),
      _PaletteCmd(app.yolo ? Icons.flash_off : Icons.bolt,
          app.yolo ? 'Disable YOLO' : 'Enable YOLO', () async => app.setYolo(!app.yolo)),
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
      _PaletteCmd(Icons.dns_outlined, 'Switch gateway', () async => app.disconnect()),
    ];

    return SafeArea(
      child: ListView(
        shrinkWrap: true,
        padding: const EdgeInsets.only(bottom: 8),
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
            child: Text('commands',
                style: TextStyle(fontWeight: FontWeight.bold, color: cs.primary)),
          ),
          for (final c in cmds)
            ListTile(
              dense: true,
              leading: Icon(c.icon, color: cs.secondary),
              title: Text(c.label),
              onTap: () {
                Navigator.pop(context);
                c.run();
              },
            ),
        ],
      ),
    );
  }
}
