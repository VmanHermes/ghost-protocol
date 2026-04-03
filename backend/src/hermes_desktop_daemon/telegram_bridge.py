from __future__ import annotations

import asyncio
import contextlib
import json
import urllib.parse
import urllib.request
from dataclasses import dataclass, field

from .config import Settings
from .events import EventBus, EventSubscription


@dataclass(slots=True)
class TelegramBridge:
    settings: Settings
    bus: EventBus
    _task: asyncio.Task | None = None
    _subscription: EventSubscription | None = None
    _last_status_by_run: dict[str, str] = field(default_factory=dict)

    async def start(self) -> None:
        if not self.settings.telegram_enabled:
            return
        self._subscription = await self.bus.subscribe()
        self._task = asyncio.create_task(self._run())

    async def stop(self) -> None:
        if self._task:
            self._task.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await self._task
        if self._subscription:
            await self.bus.unsubscribe(self._subscription)

    async def _run(self) -> None:
        assert self._subscription is not None
        while True:
            event = await self._subscription.queue.get()
            message = self._event_to_message(event)
            if message:
                await asyncio.to_thread(self._send_text, message)

    def _event_to_message(self, event: dict) -> str | None:
        event_type = event.get('type')
        run_id = event.get('runId') or ''
        summary = str(event.get('summary') or '').strip()
        payload = event.get('payload') or {}

        if event_type == 'run_started':
            return f"Hermes Desktop: Starting run {run_id[:8]}…"
        if event_type == 'run_status_changed':
            if not summary or self._last_status_by_run.get(run_id) == summary:
                return None
            self._last_status_by_run[run_id] = summary
            if summary in {'Planning…', 'Completed'} or summary.startswith('Running tool:') or summary.startswith('Awaiting approval') or summary.startswith('Failed'):
                return f"Hermes Desktop: {summary}"
            return None
        if event_type == 'approval_requested':
            return f"Hermes Desktop approval needed: {summary}. Use the desktop app for full review."
        if event_type == 'run_finished':
            status = payload.get('status', 'done')
            return f"Hermes Desktop run finished: {status}"
        if event_type == 'message_created' and payload.get('role') == 'assistant' and event.get('runId'):
            content = str(payload.get('content') or '').strip()
            if not content:
                return None
            compact = content if len(content) <= 500 else content[:497] + '...'
            return f"Hermes Desktop result:\n\n{compact}"
        return None

    def _send_text(self, text: str) -> None:
        assert self.settings.telegram_bot_token is not None
        assert self.settings.telegram_chat_id is not None
        url = f"https://api.telegram.org/bot{self.settings.telegram_bot_token}/sendMessage"
        payload = urllib.parse.urlencode({
            'chat_id': self.settings.telegram_chat_id,
            'text': text,
            'disable_web_page_preview': 'true',
        }).encode()
        req = urllib.request.Request(url, data=payload, method='POST')
        with urllib.request.urlopen(req, timeout=20) as resp:
            json.loads(resp.read().decode())
