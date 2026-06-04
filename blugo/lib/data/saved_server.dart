/// A saved blumi gateway the user can connect to, identified by a friendly
/// [name] (defaults to the machine hostname). Persisted as JSON in
/// shared_preferences so blugo can connect to several machines.
class SavedServer {
  /// Stable id — `host:port`, so re-adding the same endpoint updates in place.
  final String id;
  final String name;
  final String host;
  final int port;

  /// The last good bearer token, reused for silent reconnect. When it stops
  /// working the connect screen asks for the password again.
  final String? token;

  const SavedServer({
    required this.id,
    required this.name,
    required this.host,
    required this.port,
    this.token,
  });

  factory SavedServer.create({
    required String name,
    required String host,
    required int port,
    String? token,
  }) =>
      SavedServer(
        id: '$host:$port',
        name: name,
        host: host,
        port: port,
        token: token,
      );

  /// Base URL for the API — accepts a bare host (`10.0.0.61`) or a full URL.
  String get base => host.startsWith('http') ? host : 'http://$host:$port';

  /// `host:port` for display under the name.
  String get endpoint => host.startsWith('http') ? host : '$host:$port';

  SavedServer copyWith({String? name, String? token}) => SavedServer(
        id: id,
        name: name ?? this.name,
        host: host,
        port: port,
        token: token ?? this.token,
      );

  Map<String, dynamic> toJson() => {
        'id': id,
        'name': name,
        'host': host,
        'port': port,
        if (token != null) 'token': token,
      };

  factory SavedServer.fromJson(Map<String, dynamic> j) {
    final host = j['host'] as String? ?? '';
    final port = (j['port'] as num?)?.toInt() ?? 7777;
    return SavedServer(
      id: j['id'] as String? ?? '$host:$port',
      name: j['name'] as String? ?? (host.isNotEmpty ? host : 'blumi'),
      host: host,
      port: port,
      token: j['token'] as String?,
    );
  }
}
