from __future__ import annotations

import json
from datetime import datetime, timezone
from typing import Any
from uuid import uuid4

from .contracts import ConversationRecord, EventEnvelope, MessageRecord, RunRecord
from .db import Database


def now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


class HermesStore:
    def __init__(self, db: Database):
        self.db = db

    def create_conversation(self, title: str | None = None) -> ConversationRecord:
        conversation_id = str(uuid4())
        ts = now_iso()
        with self.db.connection() as conn:
            conn.execute(
                'INSERT INTO conversations (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)',
                (conversation_id, title, ts, ts),
            )
        return ConversationRecord(id=conversation_id, title=title, createdAt=ts, updatedAt=ts)

    def list_conversations(self) -> list[ConversationRecord]:
        with self.db.connection() as conn:
            rows = conn.execute('SELECT id, title, created_at, updated_at FROM conversations ORDER BY updated_at DESC').fetchall()
        return [ConversationRecord(id=row['id'], title=row['title'], createdAt=row['created_at'], updatedAt=row['updated_at']) for row in rows]

    def get_conversation(self, conversation_id: str) -> ConversationRecord | None:
        with self.db.connection() as conn:
            row = conn.execute('SELECT id, title, created_at, updated_at FROM conversations WHERE id = ?', (conversation_id,)).fetchone()
        if not row:
            return None
        return ConversationRecord(id=row['id'], title=row['title'], createdAt=row['created_at'], updatedAt=row['updated_at'])

    def append_message(self, conversation_id: str, role: str, content: str, run_id: str | None = None) -> MessageRecord:
        message_id = str(uuid4())
        ts = now_iso()
        with self.db.connection() as conn:
            conn.execute(
                'INSERT INTO messages (id, conversation_id, role, content, created_at, run_id) VALUES (?, ?, ?, ?, ?, ?)',
                (message_id, conversation_id, role, content, ts, run_id),
            )
            conn.execute('UPDATE conversations SET updated_at = ? WHERE id = ?', (ts, conversation_id))
        return MessageRecord(id=message_id, conversationId=conversation_id, role=role, content=content, createdAt=ts, runId=run_id)

    def list_messages(self, conversation_id: str) -> list[MessageRecord]:
        with self.db.connection() as conn:
            rows = conn.execute(
                'SELECT id, conversation_id, role, content, created_at, run_id FROM messages WHERE conversation_id = ? ORDER BY created_at ASC',
                (conversation_id,),
            ).fetchall()
        return [MessageRecord(id=row['id'], conversationId=row['conversation_id'], role=row['role'], content=row['content'], createdAt=row['created_at'], runId=row['run_id']) for row in rows]

    def create_run(self, conversation_id: str, model: str | None) -> RunRecord:
        run_id = str(uuid4())
        ts = now_iso()
        with self.db.connection() as conn:
            conn.execute(
                '''INSERT INTO runs (
                id, conversation_id, status, waiting_reason, current_step, model,
                token_usage, cost_estimate, started_at, finished_at, heartbeat_at,
                cancellation_requested_at, stale_after
                ) VALUES (?, ?, 'pending', NULL, NULL, ?, 0, 0, ?, NULL, ?, NULL, NULL)''',
                (run_id, conversation_id, model, ts, ts),
            )
            conn.execute(
                '''INSERT OR REPLACE INTO run_live_projection (
                run_id, conversation_id, status, waiting_reason, current_step,
                active_agents, token_usage, cost_estimate, pending_approvals, updated_at
                ) VALUES (?, ?, 'pending', NULL, NULL, 0, 0, 0, 0, ?)''',
                (run_id, conversation_id, ts),
            )
        return RunRecord(id=run_id, conversationId=conversation_id, status='pending', model=model, startedAt=ts, heartbeatAt=ts)

    def update_run(self, run_id: str, **fields: Any) -> None:
        allowed = {
            'status': 'status',
            'waitingReason': 'waiting_reason',
            'currentStep': 'current_step',
            'model': 'model',
            'tokenUsage': 'token_usage',
            'costEstimate': 'cost_estimate',
            'finishedAt': 'finished_at',
            'heartbeatAt': 'heartbeat_at',
            'cancellationRequestedAt': 'cancellation_requested_at',
            'staleAfter': 'stale_after',
        }
        updates = []
        values = []
        live_updates = []
        live_values = []
        for key, column in allowed.items():
            if key in fields:
                updates.append(f'{column} = ?')
                values.append(fields[key])
                if column in {'status', 'waiting_reason', 'current_step', 'token_usage', 'cost_estimate'}:
                    live_updates.append(f'{column} = ?')
                    live_values.append(fields[key])
        if not updates:
            return
        with self.db.connection() as conn:
            conn.execute(f"UPDATE runs SET {', '.join(updates)} WHERE id = ?", (*values, run_id))
            if live_updates:
                conn.execute(f"UPDATE run_live_projection SET {', '.join(live_updates)}, updated_at = ? WHERE run_id = ?", (*live_values, now_iso(), run_id))

    def get_run(self, run_id: str) -> RunRecord | None:
        with self.db.connection() as conn:
            row = conn.execute('SELECT * FROM runs WHERE id = ?', (run_id,)).fetchone()
        if not row:
            return None
        return RunRecord(
            id=row['id'], conversationId=row['conversation_id'], status=row['status'], waitingReason=row['waiting_reason'],
            currentStep=row['current_step'], model=row['model'], tokenUsage=row['token_usage'], costEstimate=row['cost_estimate'],
            startedAt=row['started_at'], finishedAt=row['finished_at'], heartbeatAt=row['heartbeat_at'],
            cancellationRequestedAt=row['cancellation_requested_at'], staleAfter=row['stale_after']
        )

    def list_runs(self) -> list[RunRecord]:
        with self.db.connection() as conn:
            rows = conn.execute('SELECT id FROM runs ORDER BY started_at DESC').fetchall()
        return [self.get_run(row['id']) for row in rows if self.get_run(row['id']) is not None]

    def append_event(self, event: EventEnvelope) -> EventEnvelope:
        with self.db.connection() as conn:
            cursor = conn.execute(
                '''INSERT INTO events (
                event_id, type, ts, conversation_id, run_id, agent_id, step_id,
                tool_call_id, artifact_id, approval_id, causation_id, correlation_id,
                visibility, payload_version, summary, payload_json
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)''',
                (
                    event.eventId, event.type, event.ts, event.conversationId, event.runId, event.agentId, event.stepId,
                    event.toolCallId, event.artifactId, event.approvalId, event.causationId, event.correlationId,
                    event.visibility, event.payloadVersion, event.summary, json.dumps(event.payload, ensure_ascii=False),
                ),
            )
            event.seq = int(cursor.lastrowid)
            if event.runId:
                conn.execute(
                    'INSERT INTO run_timeline_projection (run_id, seq, event_type, summary, payload_json, created_at) VALUES (?, ?, ?, ?, ?, ?)',
                    (event.runId, event.seq, event.type, event.summary, json.dumps(event.payload, ensure_ascii=False), event.ts),
                )
        return event

    def list_events(self, *, after_seq: int = 0, conversation_id: str | None = None, run_id: str | None = None, limit: int = 500) -> list[dict[str, Any]]:
        clauses = ['seq > ?']
        values: list[Any] = [after_seq]
        if conversation_id:
            clauses.append('conversation_id = ?')
            values.append(conversation_id)
        if run_id:
            clauses.append('run_id = ?')
            values.append(run_id)
        query = f"SELECT * FROM events WHERE {' AND '.join(clauses)} ORDER BY seq ASC LIMIT ?"
        values.append(limit)
        with self.db.connection() as conn:
            rows = conn.execute(query, values).fetchall()
        events = []
        for row in rows:
            payload = json.loads(row['payload_json']) if row['payload_json'] else {}
            events.append({
                'eventId': row['event_id'], 'type': row['type'], 'ts': row['ts'], 'seq': row['seq'],
                'conversationId': row['conversation_id'], 'runId': row['run_id'], 'agentId': row['agent_id'],
                'stepId': row['step_id'], 'toolCallId': row['tool_call_id'], 'artifactId': row['artifact_id'],
                'approvalId': row['approval_id'], 'causationId': row['causation_id'], 'correlationId': row['correlation_id'],
                'visibility': row['visibility'], 'payloadVersion': row['payload_version'], 'summary': row['summary'], 'payload': payload,
            })
        return events

    def get_run_live(self, run_id: str) -> dict[str, Any] | None:
        with self.db.connection() as conn:
            row = conn.execute('SELECT * FROM run_live_projection WHERE run_id = ?', (run_id,)).fetchone()
        return dict(row) if row else None

    def list_run_timeline(self, run_id: str) -> list[dict[str, Any]]:
        with self.db.connection() as conn:
            rows = conn.execute('SELECT * FROM run_timeline_projection WHERE run_id = ? ORDER BY seq ASC', (run_id,)).fetchall()
        return [dict(row) | {'payload': json.loads(row['payload_json'])} for row in rows]
