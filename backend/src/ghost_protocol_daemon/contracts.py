from __future__ import annotations

from datetime import datetime
from typing import Any, Literal
from uuid import uuid4

from pydantic import BaseModel, Field

Visibility = Literal['internal', 'operator', 'user', 'telegram_summary']
RunStatus = Literal['pending', 'running', 'waiting', 'done', 'error', 'cancelled']
ApprovalStatus = Literal['pending', 'approved', 'rejected', 'expired']
StepType = Literal['llm', 'tool', 'decision', 'approval', 'system']
ArtifactType = Literal['file', 'screenshot', 'patch', 'report', 'log']
TerminalSessionMode = Literal['agent', 'project', 'rescue_shell']
TerminalSessionStatus = Literal['created', 'running', 'exited', 'terminated', 'error']
TerminalStream = Literal['stdout', 'stderr', 'system']


class EventEnvelope(BaseModel):
    eventId: str = Field(default_factory=lambda: str(uuid4()))
    type: str
    ts: str = Field(default_factory=lambda: datetime.utcnow().isoformat() + 'Z')
    seq: int = 0
    conversationId: str | None = None
    runId: str | None = None
    agentId: str | None = None
    stepId: str | None = None
    toolCallId: str | None = None
    artifactId: str | None = None
    approvalId: str | None = None
    causationId: str | None = None
    correlationId: str | None = None
    visibility: Visibility = 'operator'
    payloadVersion: int = 1
    summary: str
    payload: dict[str, Any] = Field(default_factory=dict)


class ConversationRecord(BaseModel):
    id: str
    title: str | None = None
    createdAt: str
    updatedAt: str


class MessageRecord(BaseModel):
    id: str
    conversationId: str
    role: Literal['user', 'assistant', 'system']
    content: str
    createdAt: str
    runId: str | None = None


class RunRecord(BaseModel):
    id: str
    conversationId: str
    status: RunStatus
    waitingReason: str | None = None
    currentStep: str | None = None
    model: str | None = None
    tokenUsage: int = 0
    costEstimate: float = 0.0
    startedAt: str
    finishedAt: str | None = None
    heartbeatAt: str | None = None
    cancellationRequestedAt: str | None = None
    staleAfter: str | None = None


class RunAttemptRecord(BaseModel):
    id: str
    runId: str
    attemptNumber: int
    status: str
    startedAt: str
    finishedAt: str | None = None
    error: str | None = None


class AgentRecord(BaseModel):
    id: str
    runId: str
    parentAgentId: str | None = None
    lineage: str
    name: str
    type: str
    status: str
    currentTask: str | None = None
    model: str | None = None
    tokenUsage: int = 0
    startedAt: str
    finishedAt: str | None = None
    heartbeatAt: str | None = None


class ApprovalRecord(BaseModel):
    id: str
    runId: str
    type: str
    payload: dict[str, Any]
    status: ApprovalStatus
    requestedAt: str
    expiresAt: str | None = None
    requestedByEventId: str | None = None
    resolvedAt: str | None = None
    resolvedBy: str | None = None
    resolutionReason: str | None = None
    scope: str | None = None


class ArtifactRecord(BaseModel):
    id: str
    runId: str
    type: ArtifactType
    path: str
    mimeType: str | None = None
    size: int | None = None
    sha256: str | None = None
    metadata: dict[str, Any] = Field(default_factory=dict)
    sourceEventId: str | None = None
    createdAt: str


class UsageRecord(BaseModel):
    id: str
    entityType: Literal['run', 'agent', 'step']
    entityId: str
    model: str | None = None
    tokensIn: int = 0
    tokensOut: int = 0
    cost: float = 0.0
    createdAt: str


class TerminalSessionRecord(BaseModel):
    id: str
    mode: TerminalSessionMode
    status: TerminalSessionStatus
    name: str | None = None
    workdir: str
    command: list[str]
    createdAt: str
    startedAt: str | None = None
    finishedAt: str | None = None
    lastChunkAt: str | None = None
    pid: int | None = None
    exitCode: int | None = None


class TerminalChunkRecord(BaseModel):
    id: int
    sessionId: str
    stream: TerminalStream
    chunk: str
    createdAt: str


class SubscriptionRequest(BaseModel):
    op: Literal['subscribe']
    conversationId: str | None = None
    runId: str | None = None
    afterSeq: int | None = None
    lastEventId: str | None = None


class TerminalSubscriptionRequest(BaseModel):
    op: Literal['subscribe_terminal']
    sessionId: str
    afterChunkId: int | None = None


class PingRequest(BaseModel):
    op: Literal['ping']
    ts: str | None = None
