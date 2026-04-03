from __future__ import annotations

import asyncio
from dataclasses import dataclass

from .contracts import EventEnvelope
from .store import HermesStore


@dataclass(eq=False, slots=True)
class EventSubscription:
    queue: asyncio.Queue
    conversation_id: str | None = None
    run_id: str | None = None

    def matches(self, event: EventEnvelope) -> bool:
        if self.conversation_id and event.conversationId != self.conversation_id:
            return False
        if self.run_id and event.runId != self.run_id:
            return False
        return True


class EventBus:
    def __init__(self, store: HermesStore):
        self.store = store
        self._subscriptions: set[EventSubscription] = set()
        self._lock = asyncio.Lock()

    async def subscribe(self, conversation_id: str | None = None, run_id: str | None = None) -> EventSubscription:
        sub = EventSubscription(queue=asyncio.Queue(), conversation_id=conversation_id, run_id=run_id)
        async with self._lock:
            self._subscriptions.add(sub)
        return sub

    async def unsubscribe(self, sub: EventSubscription) -> None:
        async with self._lock:
            self._subscriptions.discard(sub)

    async def publish(self, event: EventEnvelope) -> EventEnvelope:
        persisted = self.store.append_event(event)
        async with self._lock:
            subs = list(self._subscriptions)
        for sub in subs:
            if sub.matches(persisted):
                await sub.queue.put(persisted.model_dump())
        return persisted
