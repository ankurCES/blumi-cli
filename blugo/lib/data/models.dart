/// Rendered transcript items — the mobile mirror of the TUI's `Entry` enum.
sealed class Entry {
  const Entry();
}

class UserEntry extends Entry {
  final String text;
  const UserEntry(this.text);
}

class AssistantEntry extends Entry {
  final String text;
  const AssistantEntry(this.text);
}

class ToolEntry extends Entry {
  final String id;
  final String name;
  final String summary;
  String preview;
  String? diff;

  /// null = running, true = ok, false = failed.
  bool? ok;
  ToolEntry({
    required this.id,
    required this.name,
    required this.summary,
    this.preview = '',
    this.ok,
    this.diff,
  });
}

class NoticeEntry extends Entry {
  final String text;
  const NoticeEntry(this.text);
}

/// A stored session (id + title) from `GET /api/sessions`.
class SessionInfo {
  final String id;
  final String title;
  final String model;
  final int messageCount;
  const SessionInfo({
    required this.id,
    required this.title,
    this.model = '',
    this.messageCount = 0,
  });

  factory SessionInfo.fromMap(Map<String, dynamic> j) => SessionInfo(
        id: j['id'] as String? ?? '',
        title: j['title'] as String? ?? '',
        model: j['model'] as String? ?? '',
        messageCount: (j['message_count'] as num?)?.toInt() ?? 0,
      );
}

/// A restored message from `GET /api/messages` (transcript replay on connect).
class StoredMessage {
  final String role; // user | assistant | tool
  final String text;
  final String? toolName;
  const StoredMessage({required this.role, required this.text, this.toolName});

  factory StoredMessage.fromMap(Map<String, dynamic> j) => StoredMessage(
        role: j['role'] as String? ?? 'assistant',
        text: j['text'] as String? ?? '',
        toolName: j['tool_name'] as String?,
      );
}
