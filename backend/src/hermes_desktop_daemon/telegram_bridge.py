from __future__ import annotations

import asyncio
import contextlib
import json
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from typing import Any

from .config import Settings
from .contracts import EventEnvelope
from .events import EventBus, EventSubscription
from .runtime import HermesRuntimeAdapter
from .store import HermesStore


@dataclass(slots=True)
class TelegramBridge:
    settings: Settings
    bus: EventBus
    store: HermesStore
    runtime: HermesRuntimeAdapter
    _event_task: asyncio.Task | None = None
    _poll_task: asyncio.Task | None = None
    _subscription: EventSubscription | None = None
    _last_status_by_run: dict[str, str] = field(default_factory=dict)
    _last_update_id: int = 0

    async def start(self) -> None:
        if not self.settings.telegram_enabled:
            return
        self._subscription = await self.bus.subscribe()
        self._event_task = asyncio.create_task(self._run_events())
        self._poll_task = asyncio.create_task(self._run_polling())

    async def stop(self) -> None:
        for task in (self._event_task, self._poll_task):
            if task:
                task.cancel()
                with contextlib.suppress(asyncio.CancelledError):
                    await task
        if self._subscription:
            await self.bus.unsubscribe(self._subscription)

    async def _run_events(self) -> None:
        assert self._subscription is not None
        while True:
            event = await self._subscription.queue.get()
            message = self._event_to_message(event)
            if message:
                await asyncio.to_thread(self._send_text, message)

    async def _run_polling(self) -> None:
        while True:
            try:
                updates = await asyncio.to_thread(self._get_updates)
                for update in updates:
                    await self._handle_update(update)
            except Exception:
                await asyncio.sleep(3)
            await asyncio.sleep(1)

    def _get_updates(self) -> list[dict[str, Any]]:
        assert self.settings.telegram_bot_token is not None
        params = {
            'timeout': 20,
            'allowed_updates': json.dumps(['message']),
        }
        if self._last_update_id:
            params['offset'] = self._last_update_id + 1
        url = f"https://api.telegram.org/bot{self.settings.telegram_bot_token}/getUpdates?{urllib.parse.urlencode(params)}"
        with urllib.request.urlopen(url, timeout=30) as resp:
            payload = json.loads(resp.read().decode())
        results = payload.get('result', []) if isinstance(payload, dict) else []
        if results:
            self._last_update_id = max(int(item.get('update_id', 0)) for item in results)
        return results

    async def _handle_update(self, update: dict[str, Any]) -> None:
        message = update.get('message') or {}
        chat = message.get('chat') or {}
        from_user = message.get('from') or {}
        text = (message.get('text') or '').strip()
        if not text:
            return
        if str(chat.get('id')) != str(self.settings.telegram_chat_id):
            return
        if from_user.get('is_bot'):
            return

        if text.startswith('/approve') or text.startswith('/deny'):
            await self._handle_approval_command(text)
            return

        conversation = self._get_or_create_telegram_conversation()
        existing_messages = self.store.list_messages(conversation.id)
        if not existing_messages or existing_messages[-1].content != text or existing_messages[-1].role != 'user':
            self.store.append_message(conversation.id, 'user', text)
        await self.runtime.start_run(conversation.id, text)

    async def _handle_approval_command(self, text: str) -> None:
        pending = self.store.list_pending_approvals()
        if not pending:
            await asyncio.to_thread(self._send_text, 'No pending approvals.')
            return
        approval = pending[0]
        if text.startswith('/approve'):
            status = 'approved'
            gateway_choice = 'once'
            ack = 'Approved. The run is resuming…'
        else:
            status = 'rejected'
            gateway_choice = 'deny'
            ack = 'Denied. The run will remain blocked or fail safely.'
        record = self.store.resolve_approval(approval.id, status, 'telegram-bridge')
        if record is None:
            await asyncio.to_thread(self._send_text, 'Approval no longer exists.')
            return
        try:
            from tools.approval import resolve_gateway_approval
            resolve_gateway_approval(record.runId, gateway_choice, resolve_all=False)
        except Exception:
            pass
        await self.bus.publish(EventEnvelope(
            type='approval_resolved',
            runId=record.runId,
            approvalId=record.id,
            visibility='user',
            summary='Approval resolved',
            payload={'approvalId': record.id, 'status': record.status, 'resolvedBy': 'telegram-bridge'},
        ))
        await asyncio.to_thread(self._send_text, ack)

    def _get_or_create_telegram_conversation(self):
        title = 'Telegram Home'
        for conversation in self.store.list_conversations():
            if conversation.title == title:
                return conversation
        return self.store.create_conversation(title)

    def _event_to_message(self, event: dict) -> str | None:
        event_type = event.get('type')
        run_id = event.get('runId') or ''
        summary = str(event.get('summary') or '').strip()
        payload = event.get('payload') or {}

        if event_type == 'run_started':
            return f"Starting run {run_id[:8]}…"
        if event_type == 'run_status_changed':
            if not summary or self._last_status_by_run.get(run_id) == summary:
                return None
            self._last_status_by_run[run_id] = summary
            if summary in {'Planning…', 'Completed', 'Awaiting approval…'} or summary.startswith('Running tool:') or summary.startswith('Failed'):
                return summary
            return None
        if event_type == 'approval_requested':
            return f"Approval needed: {summary}. Reply /approve or /deny. Use the desktop app for full detail."
        if event_type == 'run_finished':
            status = payload.get('status', 'done')
            return f"Run finished: {status}"
        if event_type == 'message_created' and payload.get('role') == 'assistant' and event.get('runId'):
            content = str(payload.get('content') or '').strip()
            if not content:
                return None
            compact = content if len(content) <= 500 else content[:497] + '...'
            return compact
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
