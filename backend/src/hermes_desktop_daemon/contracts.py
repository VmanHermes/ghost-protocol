from __future__ import annotations

from datetime import datetime
from typing import Any, Literal
from uuid import uuid4

from pydantic import BaseModel, Field

Visibility = Literal['internal', 'operator', 'user', 'telegram_summary']
RunStatus = Literal['pending', 'running', 'waiting', 'done', 'error', 'cancelled']


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


class SubscriptionRequest(BaseModel):
    op: Literal['subscribe']
    conversationId: str | None = None
    runId: str | None = None
    afterSeq: int | None = None
    lastEventId: str | None = None


class PingRequest(BaseModel):
    op: Literal['ping']
    ts: str | None = None
