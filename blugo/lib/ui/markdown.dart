import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_highlight/flutter_highlight.dart';
import 'package:flutter_highlight/themes/atom-one-dark.dart';
import 'package:gpt_markdown/gpt_markdown.dart';

/// Renders assistant replies as markdown — headings, lists, bold/italic, links,
/// tables, inline code, and fenced code blocks with syntax highlighting —
/// matching the TUI/web. Tolerates partial markdown while streaming.
class BlumiMarkdown extends StatelessWidget {
  final String text;
  const BlumiMarkdown(this.text, {super.key});

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return GptMarkdown(
      text,
      style: TextStyle(color: cs.onSurface, fontSize: 14.5, height: 1.35),
      followLinkColor: true,
      codeBuilder: (context, name, code, closed) => _CodeBlock(name, code),
      highlightBuilder: (context, inline, style) => Container(
        padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1),
        decoration: BoxDecoration(
          color: cs.onSurface.withValues(alpha: 0.1),
          borderRadius: BorderRadius.circular(4),
        ),
        child: Text(inline,
            style: style.copyWith(fontFamily: 'monospace', fontSize: 13)),
      ),
    );
  }
}

/// A fenced code block: language label + copy button + horizontally scrollable,
/// syntax-highlighted body (atom-one-dark).
class _CodeBlock extends StatelessWidget {
  final String name;
  final String code;
  const _CodeBlock(this.name, this.code);

  // highlight.js language ids we trust; anything else → auto-detect (null),
  // which never throws on an unknown language.
  static const _known = {
    'dart', 'rust', 'python', 'javascript', 'typescript', 'bash', 'json',
    'yaml', 'toml', 'go', 'java', 'kotlin', 'swift', 'c', 'cpp', 'csharp',
    'ruby', 'php', 'html', 'xml', 'css', 'scss', 'sql', 'markdown', 'diff',
    'dockerfile', 'makefile', 'ini', 'lua', 'r', 'scala', 'perl', 'haskell',
    'elixir', 'clojure', 'plaintext',
  };
  static const _alias = {
    'py': 'python', 'js': 'javascript', 'ts': 'typescript', 'sh': 'bash',
    'shell': 'bash', 'zsh': 'bash', 'yml': 'yaml', 'md': 'markdown',
    'rs': 'rust', 'c++': 'cpp', 'c#': 'csharp', 'rb': 'ruby', 'kt': 'kotlin',
  };

  String? get _lang {
    final n = name.trim().toLowerCase();
    final id = _alias[n] ?? n;
    return _known.contains(id) ? id : null;
  }

  @override
  Widget build(BuildContext context) {
    final label = name.trim().isEmpty ? 'code' : name.trim();
    return Container(
      width: double.infinity,
      margin: const EdgeInsets.symmetric(vertical: 6),
      decoration: BoxDecoration(
        color: const Color(0xFF282C34), // atom-one-dark background
        borderRadius: BorderRadius.circular(8),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(children: [
            const SizedBox(width: 12),
            Text(label,
                style: const TextStyle(
                    fontSize: 11,
                    fontFamily: 'monospace',
                    color: Colors.white54)),
            const Spacer(),
            IconButton(
              tooltip: 'Copy',
              visualDensity: VisualDensity.compact,
              icon: const Icon(Icons.copy, size: 15, color: Colors.white54),
              onPressed: () => Clipboard.setData(ClipboardData(text: code)),
            ),
          ]),
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: HighlightView(
              code,
              language: _lang,
              theme: atomOneDarkTheme,
              padding: const EdgeInsets.fromLTRB(12, 0, 12, 12),
              textStyle: const TextStyle(fontFamily: 'monospace', fontSize: 12.5),
            ),
          ),
        ],
      ),
    );
  }
}
