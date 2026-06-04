import 'dart:convert';
import 'package:flutter/foundation.dart';
import 'package:nsd/nsd.dart';
import 'saved_server.dart';

/// Browses the Wi-Fi/LAN for blumi gateways, which every `blumi serve`/`blumi
/// web` advertises over mDNS/DNS-SD as `_blumi._tcp`. Each found service is
/// reported as a [SavedServer] (name from the TXT record, host = resolved IPv4).
///
/// Best-effort: if mDNS isn't available (some networks block multicast) it just
/// reports nothing and the user adds gateways by IP.
class LanDiscovery {
  static const _type = '_blumi._tcp';

  Discovery? _discovery;
  final _found = <String, SavedServer>{};
  final void Function(List<SavedServer>) onChange;

  LanDiscovery(this.onChange);

  bool get active => _discovery != null;

  Future<void> start() async {
    if (_discovery != null) return;
    try {
      final d = await startDiscovery(_type,
          autoResolve: true, ipLookupType: IpLookupType.v4);
      _discovery = d;
      d.addServiceListener(_onService);
      debugPrint('blugo.mdns: discovery started for $_type');
    } catch (e) {
      _discovery = null; // multicast blocked / unsupported — silently ignore
      debugPrint('blugo.mdns: discovery failed: $e');
    }
  }

  Future<void> stop() async {
    final d = _discovery;
    _discovery = null;
    _found.clear();
    if (d != null) {
      try {
        await stopDiscovery(d);
      } catch (_) {}
    }
  }

  void _onService(Service service, ServiceStatus status) {
    final port = service.port;
    // Prefer a resolved IPv4 address; fall back to the advertised hostname.
    final host = (service.addresses != null && service.addresses!.isNotEmpty)
        ? service.addresses!.first.address
        : service.host;
    if (host == null || port == null) return;

    final name = _txt(service, 'name') ?? service.name ?? host;
    final srv = SavedServer.create(name: name, host: host, port: port);
    if (status == ServiceStatus.found) {
      _found[srv.id] = srv;
    } else {
      _found.remove(srv.id);
    }
    onChange(_found.values.toList());
  }

  String? _txt(Service s, String key) {
    final v = s.txt?[key];
    if (v == null) return null;
    try {
      final t = utf8.decode(v).trim();
      return t.isEmpty ? null : t;
    } catch (_) {
      return null;
    }
  }
}
