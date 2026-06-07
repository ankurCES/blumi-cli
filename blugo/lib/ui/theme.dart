import 'package:flutter/material.dart';
import 'kit/tokens.dart';

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
      surfaceContainerHighest: Color.lerp(surface, fg, 0.06)!,
      outline: fgDim.withValues(alpha: 0.5),
      error: error,
      onError: Colors.black,
    );

    // Design tokens for this palette. The brand ramp stays constant (identity);
    // status + muted text are tuned to the palette for AA contrast.
    final textMuted = Color.lerp(fgDim, fg, 0.28)!;
    final tokens = BlumiTokens.from(
      success: success,
      error: error,
      textMuted: textMuted,
      info: accent,
    );

    const sm = 10.0, md = 16.0, lg = 24.0;
    final hairline = fg.withValues(alpha: 0.08);
    final softFill = Color.lerp(surface, bg, 0.35)!;

    final base = ThemeData(
      useMaterial3: true,
      colorScheme: scheme,
      scaffoldBackgroundColor: bg,
      canvasColor: bg,
      splashFactory: InkSparkle.splashFactory,
      dividerColor: hairline,
      textTheme: Typography.whiteMountainView.apply(
        bodyColor: fg,
        displayColor: fg,
      ),
    );

    return base.copyWith(
      extensions: <ThemeExtension<dynamic>>[tokens],
      dividerTheme: DividerThemeData(
        color: hairline,
        thickness: 1,
        space: 1,
      ),
      cardTheme: CardThemeData(
        color: surface,
        elevation: 0,
        margin: EdgeInsets.zero,
        clipBehavior: Clip.antiAlias,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(md),
          side: BorderSide(color: hairline),
        ),
      ),
      appBarTheme: AppBarThemeData(
        backgroundColor: bg,
        foregroundColor: fg,
        elevation: 0,
        scrolledUnderElevation: 0,
        centerTitle: false,
        titleTextStyle: TextStyle(
            color: fg, fontSize: 18, fontWeight: FontWeight.w700),
      ),
      inputDecorationTheme: InputDecorationThemeData(
        filled: true,
        fillColor: softFill,
        isDense: true,
        contentPadding:
            const EdgeInsets.symmetric(horizontal: 14, vertical: 13),
        hintStyle: TextStyle(color: textMuted.withValues(alpha: 0.8)),
        labelStyle: TextStyle(color: textMuted),
        prefixIconColor: textMuted,
        suffixIconColor: textMuted,
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(sm),
          borderSide: BorderSide(color: hairline),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(sm),
          borderSide: BorderSide(color: hairline),
        ),
        focusedBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(sm),
          borderSide: BorderSide(color: primary, width: 1.6),
        ),
        errorBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(sm),
          borderSide: BorderSide(color: error),
        ),
        focusedErrorBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(sm),
          borderSide: BorderSide(color: error, width: 1.6),
        ),
      ),
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          backgroundColor: primary,
          foregroundColor: Colors.black,
          textStyle:
              const TextStyle(fontWeight: FontWeight.w700, fontSize: 15),
          padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 13),
          shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(sm + 2)),
        ),
      ),
      textButtonTheme: TextButtonThemeData(
        style: TextButton.styleFrom(
          foregroundColor: primary,
          textStyle: const TextStyle(fontWeight: FontWeight.w600),
          shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(sm)),
        ),
      ),
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          foregroundColor: fg,
          side: BorderSide(color: fg.withValues(alpha: 0.18)),
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
          shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(sm + 2)),
        ),
      ),
      iconButtonTheme: IconButtonThemeData(
        style: IconButton.styleFrom(foregroundColor: fg),
      ),
      chipTheme: ChipThemeData(
        backgroundColor: softFill,
        selectedColor: primary.withValues(alpha: 0.18),
        side: BorderSide(color: hairline),
        labelStyle: TextStyle(color: fg, fontSize: 12.5),
        secondaryLabelStyle: TextStyle(color: primary, fontSize: 12.5),
        shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(sm)),
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
      ),
      tabBarTheme: TabBarThemeData(
        labelColor: primary,
        unselectedLabelColor: textMuted,
        indicatorColor: primary,
        indicatorSize: TabBarIndicatorSize.label,
        dividerColor: Colors.transparent,
        labelStyle: const TextStyle(fontWeight: FontWeight.w700, fontSize: 13),
        unselectedLabelStyle: const TextStyle(fontSize: 13),
      ),
      dialogTheme: DialogThemeData(
        backgroundColor: surface,
        surfaceTintColor: Colors.transparent,
        elevation: 0,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(lg),
          side: BorderSide(color: hairline),
        ),
        titleTextStyle: TextStyle(
            color: fg, fontSize: 18, fontWeight: FontWeight.w700),
        contentTextStyle: TextStyle(color: fg, fontSize: 14, height: 1.4),
      ),
      bottomSheetTheme: BottomSheetThemeData(
        backgroundColor: surface,
        surfaceTintColor: Colors.transparent,
        modalBackgroundColor: surface,
        elevation: 0,
        showDragHandle: false,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.vertical(top: Radius.circular(lg)),
        ),
      ),
      listTileTheme: ListTileThemeData(
        iconColor: textMuted,
        textColor: fg,
        shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(sm)),
      ),
      snackBarTheme: SnackBarThemeData(
        backgroundColor: Color.lerp(surface, fg, 0.06)!,
        contentTextStyle: TextStyle(color: fg),
        actionTextColor: primary,
        behavior: SnackBarBehavior.floating,
        shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(sm + 2)),
      ),
      progressIndicatorTheme: ProgressIndicatorThemeData(
        color: primary,
        linearTrackColor: hairline,
        circularTrackColor: hairline,
      ),
      switchTheme: SwitchThemeData(
        thumbColor: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.selected) ? primary : textMuted,
        ),
        trackColor: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.selected)
              ? primary.withValues(alpha: 0.4)
              : softFill,
        ),
        trackOutlineColor: WidgetStateProperty.all(hairline),
      ),
      sliderTheme: SliderThemeData(
        activeTrackColor: primary,
        inactiveTrackColor: hairline,
        thumbColor: primary,
        overlayColor: primary.withValues(alpha: 0.16),
      ),
      tooltipTheme: TooltipThemeData(
        decoration: BoxDecoration(
          color: Color.lerp(surface, fg, 0.1)!,
          borderRadius: BorderRadius.circular(sm),
        ),
        textStyle: TextStyle(color: fg, fontSize: 12),
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
