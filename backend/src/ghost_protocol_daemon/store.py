from __future__ import annotations

import json
from datetime import datetime, timezone
from typing import Any
from uuid import uuid4

from .contracts import (
    AgentRecord,
    ApprovalRecord,
    ArtifactRecord,
    ConversationRecord,
    EventEnvelope,
    MessageRecord,
    RunAttemptRecord,
    RunRecord,
    TerminalChunkRecord,
    TerminalSessionMode,
    TerminalSessionRecord,
    UsageRecord,
)
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

    def create_run_attempt(self, run_id: str, attempt_number: int = 1, status: str = 'running') -> RunAttemptRecord:
        attempt_id = str(uuid4())
        ts = now_iso()
        with self.db.connection() as conn:
            conn.execute(
                'INSERT INTO run_attempts (id, run_id, attempt_number, status, started_at, finished_at, error) VALUES (?, ?, ?, ?, ?, NULL, NULL)',
                (attempt_id, run_id, attempt_number, status, ts),
            )
        return RunAttemptRecord(id=attempt_id, runId=run_id, attemptNumber=attempt_number, status=status, startedAt=ts)

    def finish_run_attempt(self, attempt_id: str, status: str, error: str | None = None) -> None:
        with self.db.connection() as conn:
            conn.execute(
                'UPDATE run_attempts SET status = ?, finished_at = ?, error = ? WHERE id = ?',
                (status, now_iso(), error, attempt_id),
            )

    def list_run_attempts(self, run_id: str) -> list[RunAttemptRecord]:
        with self.db.connection() as conn:
            rows = conn.execute('SELECT * FROM run_attempts WHERE run_id = ? ORDER BY attempt_number ASC', (run_id,)).fetchall()
        return [RunAttemptRecord(id=row['id'], runId=row['run_id'], attemptNumber=row['attempt_number'], status=row['status'], startedAt=row['started_at'], finishedAt=row['finished_at'], error=row['error']) for row in rows]

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
        for key, column in allowed.items():
            if key in fields:
                updates.append(f'{column} = ?')
                values.append(fields[key])
        if not updates:
            return
        with self.db.connection() as conn:
            conn.execute(f"UPDATE runs SET {', '.join(updates)} WHERE id = ?", (*values, run_id))

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
        results: list[RunRecord] = []
        for row in rows:
            item = self.get_run(row['id'])
            if item:
                results.append(item)
        return results

    def upsert_agent(
        self,
        *,
        agent_id: str,
        run_id: str,
        lineage: str,
        name: str,
        agent_type: str,
        status: str,
        current_task: str | None = None,
        model: str | None = None,
        token_usage: int = 0,
        parent_agent_id: str | None = None,
        finished_at: str | None = None,
    ) -> AgentRecord:
        ts = now_iso()
        with self.db.connection() as conn:
            existing = conn.execute('SELECT started_at FROM agents WHERE id = ?', (agent_id,)).fetchone()
            started_at = existing['started_at'] if existing else ts
            conn.execute(
                '''INSERT OR REPLACE INTO agents (
                id, run_id, parent_agent_id, lineage, name, type, status, current_task,
                model, token_usage, started_at, finished_at, heartbeat_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)''',
                (agent_id, run_id, parent_agent_id, lineage, name, agent_type, status, current_task, model, token_usage, started_at, finished_at, ts),
            )
        return AgentRecord(id=agent_id, runId=run_id, parentAgentId=parent_agent_id, lineage=lineage, name=name, type=agent_type, status=status, currentTask=current_task, model=model, tokenUsage=token_usage, startedAt=started_at, finishedAt=finished_at, heartbeatAt=ts)

    def list_agents(self, *, active_only: bool = False) -> list[AgentRecord]:
        query = 'SELECT * FROM agents'
        if active_only:
            query += " WHERE status IN ('running', 'waiting')"
        query += ' ORDER BY started_at DESC'
        with self.db.connection() as conn:
            rows = conn.execute(query).fetchall()
        return [AgentRecord(id=row['id'], runId=row['run_id'], parentAgentId=row['parent_agent_id'], lineage=row['lineage'], name=row['name'], type=row['type'], status=row['status'], currentTask=row['current_task'], model=row['model'], tokenUsage=row['token_usage'], startedAt=row['started_at'], finishedAt=row['finished_at'], heartbeatAt=row['heartbeat_at']) for row in rows]

    def create_approval(self, run_id: str, approval_type: str, payload: dict[str, Any], requested_by_event_id: str | None = None) -> ApprovalRecord:
        approval_id = str(uuid4())
        ts = now_iso()
        with self.db.connection() as conn:
            conn.execute(
                'INSERT INTO approvals (id, run_id, type, payload_json, status, requested_at, requested_by_event_id) VALUES (?, ?, ?, ?, ?, ?, ?)',
                (approval_id, run_id, approval_type, json.dumps(payload, ensure_ascii=False), 'pending', ts, requested_by_event_id),
            )
        return ApprovalRecord(id=approval_id, runId=run_id, type=approval_type, payload=payload, status='pending', requestedAt=ts, requestedByEventId=requested_by_event_id)

    def resolve_approval(self, approval_id: str, status: str, resolved_by: str, reason: str | None = None) -> ApprovalRecord | None:
        with self.db.connection() as conn:
            row = conn.execute('SELECT * FROM approvals WHERE id = ?', (approval_id,)).fetchone()
            if not row:
                return None
            resolved_at = now_iso()
            conn.execute(
                'UPDATE approvals SET status = ?, resolved_at = ?, resolved_by = ?, resolution_reason = ? WHERE id = ?',
                (status, resolved_at, resolved_by, reason, approval_id),
            )
            row = conn.execute('SELECT * FROM approvals WHERE id = ?', (approval_id,)).fetchone()
        return self._approval_from_row(row)

    def list_pending_approvals(self) -> list[ApprovalRecord]:
        with self.db.connection() as conn:
            rows = conn.execute("SELECT * FROM approvals WHERE status = 'pending' ORDER BY requested_at ASC").fetchall()
        return [self._approval_from_row(row) for row in rows]

    def _approval_from_row(self, row) -> ApprovalRecord:
        return ApprovalRecord(
            id=row['id'], runId=row['run_id'], type=row['type'], payload=json.loads(row['payload_json']), status=row['status'],
            requestedAt=row['requested_at'], expiresAt=row['expires_at'], requestedByEventId=row['requested_by_event_id'],
            resolvedAt=row['resolved_at'], resolvedBy=row['resolved_by'], resolutionReason=row['resolution_reason'], scope=row['scope']
        )

    def create_usage_record(self, entity_type: str, entity_id: str, model: str | None, tokens_in: int, tokens_out: int, cost: float) -> UsageRecord:
        usage_id = str(uuid4())
        ts = now_iso()
        with self.db.connection() as conn:
            conn.execute(
                'INSERT INTO usage_records (id, entity_type, entity_id, model, tokens_in, tokens_out, cost, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)',
                (usage_id, entity_type, entity_id, model, tokens_in, tokens_out, cost, ts),
            )
        return UsageRecord(id=usage_id, entityType=entity_type, entityId=entity_id, model=model, tokensIn=tokens_in, tokensOut=tokens_out, cost=cost, createdAt=ts)

    def list_usage_records(self, *, entity_type: str | None = None, entity_id: str | None = None) -> list[UsageRecord]:
        clauses = ['1=1']
        values: list[Any] = []
        if entity_type:
            clauses.append('entity_type = ?')
            values.append(entity_type)
        if entity_id:
            clauses.append('entity_id = ?')
            values.append(entity_id)
        with self.db.connection() as conn:
            rows = conn.execute(f"SELECT * FROM usage_records WHERE {' AND '.join(clauses)} ORDER BY created_at DESC", values).fetchall()
        return [UsageRecord(id=row['id'], entityType=row['entity_type'], entityId=row['entity_id'], model=row['model'], tokensIn=row['tokens_in'], tokensOut=row['tokens_out'], cost=row['cost'], createdAt=row['created_at']) for row in rows]

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
            self._apply_event_projection(conn, event)
        return event

    def _apply_event_projection(self, conn, event: EventEnvelope) -> None:
        if not event.runId:
            return
        current = conn.execute('SELECT * FROM run_live_projection WHERE run_id = ?', (event.runId,)).fetchone()
        if current is None and event.conversationId:
            conn.execute(
                '''INSERT OR REPLACE INTO run_live_projection (
                run_id, conversation_id, status, waiting_reason, current_step, active_agents, token_usage, cost_estimate, pending_approvals, updated_at
                ) VALUES (?, ?, 'pending', NULL, NULL, 0, 0, 0, 0, ?)''',
                (event.runId, event.conversationId, event.ts),
            )
            current = conn.execute('SELECT * FROM run_live_projection WHERE run_id = ?', (event.runId,)).fetchone()
        if current is None:
            return
        status = current['status']
        waiting_reason = current['waiting_reason']
        current_step = current['current_step']
        active_agents = current['active_agents']
        token_usage = current['token_usage']
        cost_estimate = current['cost_estimate']
        pending_approvals = current['pending_approvals']

        if event.type in {'run_started', 'run_status_changed', 'run_finished', 'error'}:
            status = str(event.payload.get('status', status or 'running'))
            current_step = str(event.payload.get('currentStep') or event.payload.get('message') or event.summary)
            waiting_reason = event.payload.get('waitingReason', waiting_reason)
            if event.type == 'error':
                status = 'error'
        elif event.type == 'agent_spawned':
            active_agents += 1
        elif event.type == 'agent_updated':
            agent_status = str(event.payload.get('status', ''))
            if agent_status in {'done', 'error', 'cancelled'} and active_agents > 0:
                active_agents -= 1
        elif event.type == 'usage_recorded':
            tokens_in = int(event.payload.get('tokensIn', 0) or 0)
            tokens_out = int(event.payload.get('tokensOut', 0) or 0)
            token_usage = tokens_in + tokens_out
            cost_estimate = float(event.payload.get('cost', cost_estimate) or 0.0)
        elif event.type == 'approval_requested':
            pending_approvals += 1
            status = 'waiting'
            waiting_reason = 'approval_requested'
        elif event.type == 'approval_resolved' and pending_approvals > 0:
            pending_approvals -= 1
            waiting_reason = None
            if status == 'waiting':
                status = 'running'

        conn.execute(
            '''UPDATE run_live_projection SET
            status = ?, waiting_reason = ?, current_step = ?, active_agents = ?, token_usage = ?, cost_estimate = ?, pending_approvals = ?, updated_at = ?
            WHERE run_id = ?''',
            (status, waiting_reason, current_step, active_agents, token_usage, cost_estimate, pending_approvals, event.ts, event.runId),
        )

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

    def list_artifacts(self, run_id: str) -> list[ArtifactRecord]:
        with self.db.connection() as conn:
            rows = conn.execute('SELECT * FROM artifacts WHERE run_id = ? ORDER BY created_at ASC', (run_id,)).fetchall()
        return [ArtifactRecord(id=row['id'], runId=row['run_id'], type=row['type'], path=row['path'], mimeType=row['mime_type'], size=row['size'], sha256=row['sha256'], metadata=json.loads(row['metadata_json']), sourceEventId=row['source_event_id'], createdAt=row['created_at']) for row in rows]



    def create_terminal_session(self, *, mode: TerminalSessionMode, name: str | None, workdir: str, command: list[str]) -> TerminalSessionRecord:
        session_id = str(uuid4())
        ts = now_iso()
        with self.db.connection() as conn:
            conn.execute(
                'INSERT INTO terminal_sessions (id, mode, status, name, workdir, command_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)',
                (session_id, mode, 'created', name, workdir, json.dumps(command, ensure_ascii=False), ts),
            )
        return TerminalSessionRecord(id=session_id, mode=mode, status='created', name=name, workdir=workdir, command=command, createdAt=ts)

    def update_terminal_session(self, session_id: str, **fields: Any) -> None:
        allowed = {
            'status': 'status',
            'startedAt': 'started_at',
            'finishedAt': 'finished_at',
            'lastChunkAt': 'last_chunk_at',
            'pid': 'pid',
            'exitCode': 'exit_code',
            'name': 'name',
            'workdir': 'workdir',
            'command': 'command_json',
        }
        updates = []
        values = []
        for key, column in allowed.items():
            if key in fields:
                updates.append(f'{column} = ?')
                value = fields[key]
                if key == 'command':
                    value = json.dumps(value, ensure_ascii=False)
                values.append(value)
        if not updates:
            return
        with self.db.connection() as conn:
            conn.execute(f"UPDATE terminal_sessions SET {', '.join(updates)} WHERE id = ?", (*values, session_id))

    def terminate_incomplete_terminal_sessions(self) -> None:
        ts = now_iso()
        with self.db.connection() as conn:
            conn.execute(
                "UPDATE terminal_sessions SET status = 'terminated', finished_at = COALESCE(finished_at, ?) WHERE status IN ('created', 'running')",
                (ts,),
            )

    def get_terminal_session(self, session_id: str) -> TerminalSessionRecord | None:
        with self.db.connection() as conn:
            row = conn.execute('SELECT * FROM terminal_sessions WHERE id = ?', (session_id,)).fetchone()
        if not row:
            return None
        return self._terminal_session_from_row(row)

    def list_terminal_sessions(self) -> list[TerminalSessionRecord]:
        with self.db.connection() as conn:
            rows = conn.execute('SELECT * FROM terminal_sessions ORDER BY created_at DESC, id ASC').fetchall()
        return [self._terminal_session_from_row(row) for row in rows]

    def _terminal_session_from_row(self, row) -> TerminalSessionRecord:
        return TerminalSessionRecord(
            id=row['id'],
            mode=row['mode'],
            status=row['status'],
            name=row['name'],
            workdir=row['workdir'],
            command=json.loads(row['command_json']),
            createdAt=row['created_at'],
            startedAt=row['started_at'],
            finishedAt=row['finished_at'],
            lastChunkAt=row['last_chunk_at'],
            pid=row['pid'],
            exitCode=row['exit_code'],
        )

    def append_terminal_chunk(self, session_id: str, stream: str, chunk: str) -> TerminalChunkRecord:
        ts = now_iso()
        with self.db.connection() as conn:
            cursor = conn.execute(
                'INSERT INTO terminal_chunks (session_id, stream, chunk, created_at) VALUES (?, ?, ?, ?)',
                (session_id, stream, chunk, ts),
            )
            conn.execute('UPDATE terminal_sessions SET last_chunk_at = ? WHERE id = ?', (ts, session_id))
        return TerminalChunkRecord(id=int(cursor.lastrowid), sessionId=session_id, stream=stream, chunk=chunk, createdAt=ts)

    def list_terminal_chunks(self, session_id: str, *, after_chunk_id: int = 0, limit: int = 500) -> list[TerminalChunkRecord]:
        with self.db.connection() as conn:
            rows = conn.execute(
                'SELECT * FROM terminal_chunks WHERE session_id = ? AND id > ? ORDER BY id ASC LIMIT ?',
                (session_id, after_chunk_id, limit),
            ).fetchall()
        return [
            TerminalChunkRecord(id=row['id'], sessionId=row['session_id'], stream=row['stream'], chunk=row['chunk'], createdAt=row['created_at'])
            for row in rows
        ]

    def get_terminal_session_detail(self, session_id: str) -> dict[str, Any] | None:
        session = self.get_terminal_session(session_id)
        if not session:
            return None
        return {
            'session': session.model_dump(),
            'chunks': [item.model_dump() for item in self.list_terminal_chunks(session_id)],
        }

    def get_conversation_detail(self, conversation_id: str) -> dict[str, Any] | None:
        conversation = self.get_conversation(conversation_id)
        if not conversation:
            return None
        runs = [run.model_dump() for run in self.list_runs() if run.conversationId == conversation_id]
        return {
            'conversation': conversation.model_dump(),
            'messages': [item.model_dump() for item in self.list_messages(conversation_id)],
            'runs': runs,
        }

    def get_run_detail(self, run_id: str) -> dict[str, Any] | None:
        run = self.get_run(run_id)
        if not run:
            return None
        return {
            'run': run.model_dump(),
            'live': self.get_run_live(run_id),
            'timeline': self.list_run_timeline(run_id),
            'attempts': [item.model_dump() for item in self.list_run_attempts(run_id)],
            'agents': [item.model_dump() for item in self.list_agents(active_only=False) if item.runId == run_id],
            'approvals': [item.model_dump() for item in self.list_pending_approvals() if item.runId == run_id],
            'artifacts': [item.model_dump() for item in self.list_artifacts(run_id)],
            'usage': [item.model_dump() for item in self.list_usage_records(entity_type='run', entity_id=run_id)],
        }

    def get_system_status(self) -> dict[str, Any]:
        return {
            'activeAgents': [item.model_dump() for item in self.list_agents(active_only=True)],
            'pendingApprovals': [item.model_dump() for item in self.list_pending_approvals()],
            'recentRuns': [item.model_dump() for item in self.list_runs()[:10]],
            'activeTerminalSessions': [item.model_dump() for item in self.list_terminal_sessions() if item.status == 'running'],
        }
