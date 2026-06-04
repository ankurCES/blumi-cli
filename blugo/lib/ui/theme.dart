import 'package:flutter/material.dart';

/// A blumi palette, mirroring the TUI themes — mapped to a Material dark scheme.
class BlumiTheme {
  final String name;
  final Color primary, accent, bg, surface, fg, fgDim, success, error;
  const BlumiTheme({
    required this.name,
    required this.primary,
    required this.accent,
    required this.bg,
    required this.surface,
    required this.fg,
    required this.fgDim,
    required this.success,
    required this.error,
  });

  ThemeData toThemeData() {
    final scheme = ColorScheme(
      brightness: Brightness.dark,
      primary: primary,
      onPrimary: Colors.black,
      secondary: accent,
      onSecondary: Colors.black,
      surface: surface,
      onSurface: fg,
      error: error,
      onError: Colors.black,
    );
    return ThemeData(
      useMaterial3: true,
      colorScheme: scheme,
      scaffoldBackgroundColor: bg,
      canvasColor: bg,
      dividerColor: fgDim.withValues(alpha: 0.4),
      textTheme: Typography.whiteMountainView.apply(
        bodyColor: fg,
        displayColor: fg,
      ),
    );
  }
}

const _themes = <BlumiTheme>[
  BlumiTheme(
    name: 'rose',
    primary: Color(0xFFFF4F87),
    accent: Color(0xFF68FFD6),
    bg: Color(0xFF16090E),
    surface: Color(0xFF2A1722),
    fg: Color(0xFFF6E6EC),
    fgDim: Color(0xFF8C6571),
    success: Color(0xFF4FE0A0),
    error: Color(0xFFFF5470),
  ),
  BlumiTheme(
    name: 'catppuccin',
    primary: Color(0xFFCBA6F7),
    accent: Color(0xFF94E2D5),
    bg: Color(0xFF181825),
    surface: Color(0xFF1E1E2E),
    fg: Color(0xFFCDD6F4),
    fgDim: Color(0xFF6C7086),
    success: Color(0xFFA6E3A1),
    error: Color(0xFFF38BA8),
  ),
  BlumiTheme(
    name: 'nord',
    primary: Color(0xFF88C0D0),
    accent: Color(0xFF8FBCBB),
    bg: Color(0xFF2E3440),
    surface: Color(0xFF3B4252),
    fg: Color(0xFFECEFF4),
    fgDim: Color(0xFF7B8494),
    success: Color(0xFFA3BE8C),
    error: Color(0xFFBF616A),
  ),
  BlumiTheme(
    name: 'dracula',
    primary: Color(0xFFBD93F9),
    accent: Color(0xFF8BE9FD),
    bg: Color(0xFF282A36),
    surface: Color(0xFF343746),
    fg: Color(0xFFF8F8F2),
    fgDim: Color(0xFF6272A4),
    success: Color(0xFF50FA7B),
    error: Color(0xFFFF5555),
  ),
  BlumiTheme(
    name: 'tokyo-night',
    primary: Color(0xFF7AA2F7),
    accent: Color(0xFF7DCFFF),
    bg: Color(0xFF1A1B26),
    surface: Color(0xFF24283B),
    fg: Color(0xFFC0CAF5),
    fgDim: Color(0xFF565F89),
    success: Color(0xFF9ECE6A),
    error: Color(0xFFF7768E),
  ),
  BlumiTheme(
    name: 'gruvbox',
    primary: Color(0xFFD3869B),
    accent: Color(0xFF8EC07C),
    bg: Color(0xFF1D2021),
    surface: Color(0xFF282828),
    fg: Color(0xFFEBDBB2),
    fgDim: Color(0xFF928374),
    success: Color(0xFFB8BB26),
    error: Color(0xFFFB4934),
  ),
];

/// All selectable themes (rose first, the default).
List<BlumiTheme> get blumiThemes => _themes;

BlumiTheme themeByName(String name) =>
    _themes.firstWhere((t) => t.name == name, orElse: () => _themes.first);
