import 'package:flutter/material.dart';
import '../state/app.dart';
import '../state/dispatch.dart';
import 'kit/kit.dart';
import 'markdown.dart';

/// The Broadcast channel: send one message to every saved node and watch each
/// node's reply land. Fan-out is phone-side (reuses each node's dispatch session).
class BroadcastScreen extends StatefulWidget {
  final AppController app;
  const BroadcastScreen(this.app, {super.key});
  @override
  State<BroadcastScreen> createState() => _BroadcastScreenState();
}

class _BroadcastScreenState extends State<BroadcastScreen> {
  final _input = TextEditingController();
  final _scroll = ScrollController();
  bool _sending = false;

  @override
  void dispose() {
    _input.dispose();
    _scroll.dispose();
    super.dispose();
  }

  Future<void> _send() async {
    final text = _input.text.trim();
    if (text.isEmpty || _sending) return;
    _input.clear();
    setState(() => _sending = true);
    try {
      await widget.app.dispatch.broadcast(text);
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Scaffold(
      appBar: AppBar(
        title: Row(children: const [
          Icon(Icons.campaign, size: 20),
          SizedBox(width: 8),
          GradientText('Broadcast',
              style: TextStyle(fontSize: 19, fontWeight: FontWeight.w800)),
        ]),
      ),
      body: SafeArea(
        child: Column(
          children: [
            Expanded(
              child: ListenableBuilder(
                listenable: widget.app.dispatch,
                builder: (context, _) {
                  final turns = widget.app.dispatch.broadcastTurns;
                  if (turns.isEmpty) {
                    return EmptyState(
                      icon: Icons.campaign_outlined,
                      message: 'Message every node at once',
                      hint:
                          'Sends to all ${widget.app.servers.length} saved gateways; replies land per node.',
                    );
                  }
                  return ListView.builder(
                    controller: _scroll,
                    padding: const EdgeInsets.fromLTRB(12, 12, 12, 8),
                    itemCount: turns.length,
                    itemBuilder: (context, i) =>
                        RepaintBoundary(child: _TurnView(turns[i])),
                  );
                },
              ),
            ),
            Container(
              decoration: BoxDecoration(
                border: Border(
                    top: BorderSide(
                        color: cs.onSurface.withValues(alpha: 0.08))),
              ),
              padding: const EdgeInsets.fromLTRB(8, 8, 8, 8),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.end,
                children: [
                  Expanded(
                    child: TextField(
                      controller: _input,
                      minLines: 1,
                      maxLines: 5,
                      textInputAction: TextInputAction.send,
                      onSubmitted: (_) => _send(),
                      decoration: const InputDecoration(
                          hintText: 'Message all nodes…'),
                    ),
                  ),
                  const SizedBox(width: 8),
                  _sending
                      ? const Padding(
                          padding: EdgeInsets.all(8), child: InlineSpinner())
                      : IconButton.filled(
                          onPressed: _send, icon: const Icon(Icons.campaign)),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _TurnView extends StatelessWidget {
  final BroadcastTurn turn;
  const _TurnView(this.turn);

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // The broadcast prompt (once).
        Container(
          margin: const EdgeInsets.symmetric(vertical: 6),
          padding: const EdgeInsets.only(left: 12, top: 6, bottom: 6, right: 4),
          decoration: BoxDecoration(
            border: Border(left: BorderSide(color: cs.secondary, width: 3)),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text('› broadcast',
                  style: TextStyle(
                      color: cs.secondary,
                      fontWeight: FontWeight.bold,
                      fontSize: 12)),
              const SizedBox(height: 3),
              SelectableText(turn.prompt),
            ],
          ),
        ),
        for (final r in turn.replies.values) _ReplyCard(r),
      ],
    );
  }
}

class _ReplyCard extends StatelessWidget {
  final NodeReply r;
  const _ReplyCard(this.r);

  @override
  Widget build(BuildContext context) {
    final t = BlumiTokens.of(context);
    final (status, trailing) = r.pending
        ? (BlumiStatus.busy, const InlineSpinner(size: 14))
        : r.error != null
            ? (BlumiStatus.err, null)
            : (BlumiStatus.ok, null);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: BlumiCard(
        padding: const EdgeInsets.all(11),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(children: [
              StatusDot(status, size: 8),
              const SizedBox(width: 7),
              Expanded(
                child: Text(r.node,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontWeight: FontWeight.w700)),
              ),
              ?trailing,
            ]),
            const SizedBox(height: 6),
            if (r.pending)
              Text('…', style: TextStyle(color: t.textMuted))
            else if (r.error != null)
              Text(r.error!, style: TextStyle(color: t.error, fontSize: 13))
            else
              BlumiMarkdown(r.text ?? ''),
          ],
        ),
      ),
    );
  }
}
