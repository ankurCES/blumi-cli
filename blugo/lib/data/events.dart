import 'dart:convert';

/// A live event from the blumi gateway — mirrors `blumi-protocol`'s `Event` enum,
/// which serializes tagged by a snake_case `type` field. Parsed from the `data:`
/// payload of each SSE frame.
sealed class BlumiEvent {
  const BlumiEvent();

  /// Parse one SSE `data:` JSON payload. Returns null on malformed JSON.
  static BlumiEvent? parse(String dataJson) {
    try {
      return fromMap(jsonDecode(dataJson) as Map<String, dynamic>);
    } catch (_) {
      return null;
    }
  }

  static BlumiEvent fromMap(Map<String, dynamic> j) {
    String s(String k) => j[k] as String? ?? '';
    int i(String k) => (j[k] as num?)?.toInt() ?? 0;
    return switch (j['type'] as String? ?? '') {
      'turn_started' => const TurnStarted(),
      'assistant_started' => AssistantStarted(s('message_id')),
      'token' => TokenEvent(s('text')),
      'thinking' => ThinkingEvent(s('text')),
      'assistant_finished' => AssistantFinished(s('message_id')),
      'tool_start' => ToolStart(id: s('id'), name: s('name'), summary: s('summary')),
      'tool_progress' => ToolProgress(s('id'), s('chunk')),
      'tool_result' => ToolResultEvent(
          id: s('id'),
          name: s('name'),
          ok: j['ok'] as bool? ?? false,
          preview: s('preview'),
        ),
      'diff' => DiffEvent(id: s('id'), path: s('path'), unified: s('unified')),
      'approval_request' => ApprovalRequest(
          requestId: s('request_id'),
          tool: s('tool'),
          summary: s('summary'),
          dangerous: j['dangerous'] as bool? ?? false,
          diff: j['diff'] as String?,
          advice: j['advice'] as String?,
        ),
      'clarify_request' => ClarifyRequest(
          requestId: s('request_id'),
          question: s('question'),
          choices: ((j['choices'] as List?) ?? [])
              .map((c) => ClarifyChoice(
                    (c as Map)['id'] as String? ?? '',
                    c['label'] as String? ?? '',
                  ))
              .toList(),
        ),
      'plan_review' => PlanReview(requestId: s('request_id'), plan: s('plan')),
      'agent_start' =>
        AgentStart(id: s('id'), agentType: s('agent_type'), task: s('task')),
      'agent_done' =>
        AgentDone(id: s('id'), ok: j['ok'] as bool? ?? false, summary: s('summary')),
      'todo_update' => TodoUpdate(((j['items'] as List?) ?? [])
          .map((t) => Todo.fromMap(t as Map<String, dynamic>))
          .toList()),
      'usage' => UsageEvent(
          input: i('input'),
          output: i('output'),
          context: i('context'),
          costUsd: (j['cost_usd'] as num?)?.toDouble(),
        ),
      'compaction' => const Compaction(),
      'done' => DoneEvent(j['reason'] as String? ?? 'completed'),
      'notice' => NoticeEvent(s('message')),
      'error' =>
        ErrorEvent(kind: s('kind'), message: s('message'), hint: j['hint'] as String?),
      'reload' => const ReloadEvent(),
      final other => UnknownEvent(other),
    };
  }
}

class TurnStarted extends BlumiEvent {
  const TurnStarted();
}

class AssistantStarted extends BlumiEvent {
  final String messageId;
  const AssistantStarted(this.messageId);
}

class TokenEvent extends BlumiEvent {
  final String text;
  const TokenEvent(this.text);
}

class ThinkingEvent extends BlumiEvent {
  final String text;
  const ThinkingEvent(this.text);
}

class AssistantFinished extends BlumiEvent {
  final String messageId;
  const AssistantFinished(this.messageId);
}

class ToolStart extends BlumiEvent {
  final String id, name, summary;
  const ToolStart({required this.id, required this.name, required this.summary});
}

class ToolProgress extends BlumiEvent {
  final String id, chunk;
  const ToolProgress(this.id, this.chunk);
}

class ToolResultEvent extends BlumiEvent {
  final String id, name, preview;
  final bool ok;
  const ToolResultEvent(
      {required this.id, required this.name, required this.ok, required this.preview});
}

class DiffEvent extends BlumiEvent {
  final String id, path, unified;
  const DiffEvent({required this.id, required this.path, required this.unified});
}

class ApprovalRequest extends BlumiEvent {
  final String requestId, tool, summary;
  final bool dangerous;
  final String? diff, advice;
  const ApprovalRequest({
    required this.requestId,
    required this.tool,
    required this.summary,
    required this.dangerous,
    this.diff,
    this.advice,
  });
}

class ClarifyChoice {
  final String id, label;
  const ClarifyChoice(this.id, this.label);
}

class ClarifyRequest extends BlumiEvent {
  final String requestId, question;
  final List<ClarifyChoice> choices;
  const ClarifyRequest(
      {required this.requestId, required this.question, required this.choices});
}

class PlanReview extends BlumiEvent {
  final String requestId, plan;
  const PlanReview({required this.requestId, required this.plan});
}

class AgentStart extends BlumiEvent {
  final String id, agentType, task;
  const AgentStart({required this.id, required this.agentType, required this.task});
}

class AgentDone extends BlumiEvent {
  final String id, summary;
  final bool ok;
  const AgentDone({required this.id, required this.ok, required this.summary});
}

enum TodoStatus { pending, inProgress, completed }

class Todo {
  final String id, content;
  final TodoStatus status;
  const Todo({required this.id, required this.content, required this.status});

  factory Todo.fromMap(Map<String, dynamic> j) => Todo(
        id: j['id'] as String? ?? '',
        content: j['content'] as String? ?? '',
        status: switch (j['status'] as String? ?? 'pending') {
          'in_progress' => TodoStatus.inProgress,
          'completed' => TodoStatus.completed,
          _ => TodoStatus.pending,
        },
      );
}

class TodoUpdate extends BlumiEvent {
  final List<Todo> items;
  const TodoUpdate(this.items);
}

class UsageEvent extends BlumiEvent {
  final int input, output, context;
  final double? costUsd;
  const UsageEvent(
      {required this.input,
      required this.output,
      required this.context,
      this.costUsd});
}

class Compaction extends BlumiEvent {
  const Compaction();
}

class DoneEvent extends BlumiEvent {
  final String reason;
  const DoneEvent(this.reason);
}

class NoticeEvent extends BlumiEvent {
  final String message;
  const NoticeEvent(this.message);
}

class ErrorEvent extends BlumiEvent {
  final String kind, message;
  final String? hint;
  const ErrorEvent({required this.kind, required this.message, this.hint});
}

class ReloadEvent extends BlumiEvent {
  const ReloadEvent();
}

class UnknownEvent extends BlumiEvent {
  final String type;
  const UnknownEvent(this.type);
}
