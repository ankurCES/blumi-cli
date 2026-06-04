import 'dart:async';
import 'dart:convert';
import 'package:shared_preferences/shared_preferences.dart';

/// Tiny stale-while-revalidate cache for API responses.
///
/// Holds raw JSON (Map/List/scalars) in memory and mirrors it to
/// shared_preferences (debounced), so views paint instantly from the
/// last-known data — even on a cold start — and only hit the network when the
/// entry is older than its TTL.
class DataCache {
  static const _prefsKey = 'blugo_cache_v1';

  final Map<String, _Entry> _mem = {};
  SharedPreferences? _prefs;
  Timer? _saveDebounce;

  /// Load the persisted cache. Safe to call once at startup.
  Future<void> init() async {
    try {
      _prefs = await SharedPreferences.getInstance();
      final raw = _prefs!.getString(_prefsKey);
      if (raw != null) {
        (jsonDecode(raw) as Map<String, dynamic>).forEach((k, e) {
          final at = DateTime.tryParse((e as Map)['at'] as String? ?? '');
          if (at != null) _mem[k] = _Entry(e['v'], at);
        });
      }
    } catch (_) {
      // corrupt/missing cache — start empty
    }
  }

  /// Last-known raw value for [key] (may be stale), or null.
  dynamic peek(String key) => _mem[key]?.value;

  /// True if [key] is cached and younger than [ttl].
  bool isFresh(String key, Duration ttl) {
    final e = _mem[key];
    return e != null && DateTime.now().difference(e.at) < ttl;
  }

  /// Store a raw JSON value and (debounced) persist.
  void put(String key, dynamic rawJson) {
    _mem[key] = _Entry(rawJson, DateTime.now());
    _scheduleSave();
  }

  /// Forget everything (e.g. on disconnect).
  void clear() {
    _mem.clear();
    _saveDebounce?.cancel();
    _prefs?.remove(_prefsKey);
  }

  void _scheduleSave() {
    _saveDebounce?.cancel();
    _saveDebounce = Timer(const Duration(milliseconds: 400), _save);
  }

  void _save() {
    final p = _prefs;
    if (p == null) return;
    try {
      final m = {
        for (final e in _mem.entries)
          e.key: {'v': e.value.value, 'at': e.value.at.toIso8601String()}
      };
      p.setString(_prefsKey, jsonEncode(m));
    } catch (_) {
      // non-encodable value slipped in — skip this save
    }
  }
}

class _Entry {
  final dynamic value;
  final DateTime at;
  _Entry(this.value, this.at);
}
