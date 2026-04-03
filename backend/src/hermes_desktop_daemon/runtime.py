from __future__ import annotations

import asyncio
import sys
from dataclasses import dataclass
from typing import Any
from uuid import uuid4

from .config import Settings
from .contracts import EventEnvelope
from .events import EventBus
from .store import HermesStore, now_iso


@dataclass(slots=True)
class RuntimeContext:
    conversation_id: str
    run_id: str
    user_message: str


class HermesRuntimeAdapter:
    def __init__(self, settings: Settings, store: HermesStore, bus: EventBus):
        self.settings = settings
        self.store = store
        self.bus = bus
        self._active_tasks: dict[str, asyncio.Task] = {}
        if str(settings.hermes_source_root) not in sys.path:
            sys.path.insert(0, str(settings.hermes_source_root))
        from run_agent import AIAgent  # type: ignore
        self._agent_cls = AIAgent

    async def start_run(self, conversation_id: str, user_message: str) -> str:
        run = self.store.create_run(conversation_id, self.settings.model)
        task = asyncio.create_task(self._run_in_background(RuntimeContext(conversation_id=conversation_id, run_id=run.id, user_message=user_message)))
        self._active_tasks[run.id] = task
        task.add_done_callback(lambda _: self._active_tasks.pop(run.id, None))
        return run.id

    async def retry_run(self, run_id: str) -> str:
        run = self.store.get_run(run_id)
        if not run:
            raise ValueError('run not found')
        messages = self.store.list_messages(run.conversationId)
        user_messages = [item for item in messages if item.role == 'user']
        if not user_messages:
            raise ValueError('no user message available for retry')
        return await self.start_run(run.conversationId, user_messages[-1].content)

    async def request_cancel(self, run_id: str) -> None:
        run = self.store.get_run(run_id)
        conversation_id = run.conversationId if run else None
        self.store.update_run(run_id, cancellationRequestedAt=now_iso(), status='cancelled', finishedAt=now_iso(), heartbeatAt=now_iso())
        await self._emit(
            type='run_status_changed', conversationId=conversation_id, runId=run_id, summary='Cancellation requested', visibility='user', payload={'status': 'cancelled'}
        )
        await self._emit(
            type='run_finished', conversationId=conversation_id, runId=run_id, summary='Cancelled', visibility='user', payload={'status': 'cancelled'}
        )
        task = self._active_tasks.get(run_id)
        if task and not task.done():
            task.cancel()

    async def _emit(self, **kwargs: Any) -> EventEnvelope:
        event = EventEnvelope(**kwargs)
        return await self.bus.publish(event)

    async def _run_in_background(self, ctx: RuntimeContext) -> None:
        attempt = self.store.create_run_attempt(ctx.run_id, attempt_number=1, status='running')
        root_agent_id = f'{ctx.run_id}:root-agent'
        self.store.upsert_agent(
            agent_id=root_agent_id,
            run_id=ctx.run_id,
            lineage='0',
            name='Hermes',
            agent_type='primary',
            status='running',
            current_task='Planning',
            model=self.settings.model,
        )
        await self._emit(
            type='run_started', conversationId=ctx.conversation_id, runId=ctx.run_id,
            visibility='operator', summary='Starting run…', payload={'status': 'running', 'attemptNumber': 1}
        )
        await self._emit(
            type='agent_spawned', conversationId=ctx.conversation_id, runId=ctx.run_id, agentId=root_agent_id,
            visibility='operator', summary='Primary agent started', payload={'name': 'Hermes', 'type': 'primary', 'status': 'running'}
        )
        self.store.update_run(ctx.run_id, status='running', heartbeatAt=now_iso(), currentStep='Planning')
        await self._emit(
            type='run_status_changed', conversationId=ctx.conversation_id, runId=ctx.run_id,
            visibility='user', summary='Planning…', payload={'status': 'running', 'currentStep': 'Planning'}
        )

        messages = self.store.list_messages(ctx.conversation_id)
        history = [{'role': m.role, 'content': m.content} for m in messages[:-1] if m.role in {'user', 'assistant', 'system'}]
        loop = asyncio.get_running_loop()
        tool_step_ids: dict[str, str] = {}

        async def publish_from_thread(event: EventEnvelope) -> None:
            await self.bus.publish(event)

        def emit_threadsafe(event: EventEnvelope) -> None:
            asyncio.run_coroutine_threadsafe(publish_from_thread(event), loop)

        def status_callback(kind: str, message: str) -> None:
            self.store.update_run(ctx.run_id, currentStep=message, heartbeatAt=now_iso())
            self.store.upsert_agent(
                agent_id=root_agent_id,
                run_id=ctx.run_id,
                lineage='0',
                name='Hermes',
                agent_type='primary',
                status='running',
                current_task=message,
                model=self.settings.model,
            )
            emit_threadsafe(EventEnvelope(
                type='agent_updated', conversationId=ctx.conversation_id, runId=ctx.run_id, agentId=root_agent_id,
                visibility='operator', summary=f'Agent status: {message}', payload={'status': 'running', 'currentTask': message}
            ))
            emit_threadsafe(EventEnvelope(
                type='run_status_changed', conversationId=ctx.conversation_id, runId=ctx.run_id,
                visibility='user', summary=message, payload={'kind': kind, 'message': message, 'status': 'running', 'currentStep': message}
            ))

        def step_callback(iteration: int, prev_tools: list[dict[str, Any]] | list[Any]) -> None:
            summary = f'Step {iteration} started'
            step_id = f'{ctx.run_id}:step:{iteration}'
            self.store.update_run(ctx.run_id, currentStep=summary, heartbeatAt=now_iso())
            emit_threadsafe(EventEnvelope(
                type='step_started', conversationId=ctx.conversation_id, runId=ctx.run_id, stepId=step_id,
                visibility='operator', summary=summary,
                payload={'iteration': iteration, 'tools': prev_tools or []}
            ))

        def tool_start_callback(tool_call_id: str, tool_name: str, args: dict[str, Any]) -> None:
            step_id = tool_step_ids.setdefault(tool_call_id, f'{ctx.run_id}:tool-step:{tool_call_id}')
            emit_threadsafe(EventEnvelope(
                type='tool_called', conversationId=ctx.conversation_id, runId=ctx.run_id,
                stepId=step_id, toolCallId=tool_call_id, visibility='operator',
                summary=f'Running tool: {tool_name}', payload={'toolName': tool_name, 'input': args}
            ))
            self.store.update_run(ctx.run_id, currentStep=f'Running tool: {tool_name}', heartbeatAt=now_iso())

        def tool_complete_callback(tool_call_id: str, tool_name: str, args: dict[str, Any], result: Any) -> None:
            step_id = tool_step_ids.setdefault(tool_call_id, f'{ctx.run_id}:tool-step:{tool_call_id}')
            payload_result = result if isinstance(result, (dict, list, str, int, float, bool)) or result is None else str(result)
            emit_threadsafe(EventEnvelope(
                type='tool_result', conversationId=ctx.conversation_id, runId=ctx.run_id,
                stepId=step_id, toolCallId=tool_call_id, visibility='operator',
                summary=f'Tool finished: {tool_name}', payload={'toolName': tool_name, 'input': args, 'output': payload_result}
            ))
            emit_threadsafe(EventEnvelope(
                type='step_finished', conversationId=ctx.conversation_id, runId=ctx.run_id,
                stepId=step_id, toolCallId=tool_call_id, visibility='operator',
                summary=f'Completed tool step: {tool_name}', payload={'toolName': tool_name}
            ))

        def run_sync() -> dict[str, Any]:
            agent = self._agent_cls(
                model=self.settings.model or '',
                quiet_mode=True,
                verbose_logging=False,
                enabled_toolsets=self.settings.enabled_toolsets or None,
                session_id=ctx.run_id,
                platform='api',
                step_callback=step_callback,
                status_callback=status_callback,
                tool_start_callback=tool_start_callback,
                tool_complete_callback=tool_complete_callback,
            )
            result = agent.run_conversation(ctx.user_message, conversation_history=history, task_id=ctx.run_id)
            result['_agent_model'] = getattr(agent, 'model', self.settings.model)
            result['_input_tokens'] = int(getattr(agent, 'session_prompt_tokens', 0) or 0)
            result['_output_tokens'] = int(getattr(agent, 'session_completion_tokens', 0) or 0)
            try:
                result['_cost_estimate'] = float(getattr(agent, 'estimated_session_cost', 0.0) or 0.0)
            except Exception:
                result['_cost_estimate'] = 0.0
            return result

        try:
            result = await asyncio.to_thread(run_sync)
            final_response = result.get('final_response') or result.get('error') or '(No response generated)'
            assistant_message = self.store.append_message(ctx.conversation_id, 'assistant', final_response, run_id=ctx.run_id)
            await self._emit(
                type='message_created', conversationId=ctx.conversation_id, runId=ctx.run_id,
                visibility='user', summary='Assistant response created',
                payload={'messageId': assistant_message.id, 'role': 'assistant', 'content': final_response}
            )
            token_in = int(result.get('_input_tokens', 0) or 0)
            token_out = int(result.get('_output_tokens', 0) or 0)
            token_usage = token_in + token_out
            cost_estimate = float(result.get('_cost_estimate', 0.0) or 0.0)
            self.store.create_usage_record('run', ctx.run_id, result.get('_agent_model'), token_in, token_out, cost_estimate)
            self.store.update_run(
                ctx.run_id,
                status='done',
                finishedAt=now_iso(),
                heartbeatAt=now_iso(),
                tokenUsage=token_usage,
                costEstimate=cost_estimate,
                model=result.get('_agent_model'),
                currentStep='Completed',
            )
            self.store.upsert_agent(
                agent_id=root_agent_id,
                run_id=ctx.run_id,
                lineage='0',
                name='Hermes',
                agent_type='primary',
                status='done',
                current_task='Completed',
                model=result.get('_agent_model'),
                token_usage=token_usage,
                finished_at=now_iso(),
            )
            self.store.finish_run_attempt(attempt.id, 'done')
            await self._emit(
                type='usage_recorded', conversationId=ctx.conversation_id, runId=ctx.run_id, agentId=root_agent_id,
                visibility='operator', summary='Usage recorded',
                payload={'entityType': 'run', 'entityId': ctx.run_id, 'model': result.get('_agent_model'), 'tokensIn': token_in, 'tokensOut': token_out, 'cost': cost_estimate}
            )
            await self._emit(
                type='agent_updated', conversationId=ctx.conversation_id, runId=ctx.run_id, agentId=root_agent_id,
                visibility='operator', summary='Primary agent completed', payload={'status': 'done', 'currentTask': 'Completed'}
            )
            await self._emit(
                type='run_finished', conversationId=ctx.conversation_id, runId=ctx.run_id,
                visibility='user', summary='Completed', payload={'status': 'done'}
            )
        except asyncio.CancelledError:
            self.store.finish_run_attempt(attempt.id, 'cancelled', error='cancelled')
            raise
        except Exception as exc:
            self.store.finish_run_attempt(attempt.id, 'error', error=str(exc))
            self.store.update_run(ctx.run_id, status='error', finishedAt=now_iso(), heartbeatAt=now_iso(), currentStep='Failed')
            self.store.upsert_agent(
                agent_id=root_agent_id,
                run_id=ctx.run_id,
                lineage='0',
                name='Hermes',
                agent_type='primary',
                status='error',
                current_task='Failed',
                model=self.settings.model,
                finished_at=now_iso(),
            )
            await self._emit(
                type='error', conversationId=ctx.conversation_id, runId=ctx.run_id, agentId=root_agent_id,
                visibility='user', summary='Failed', payload={'message': str(exc), 'status': 'error'}
            )
            await self._emit(
                type='agent_updated', conversationId=ctx.conversation_id, runId=ctx.run_id, agentId=root_agent_id,
                visibility='operator', summary='Primary agent failed', payload={'status': 'error', 'currentTask': 'Failed'}
            )
            await self._emit(
                type='run_finished', conversationId=ctx.conversation_id, runId=ctx.run_id,
                visibility='user', summary='Failed', payload={'status': 'error', 'message': str(exc)}
            )
