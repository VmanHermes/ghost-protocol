from __future__ import annotations

import asyncio
import ipaddress
import json

from aiohttp import WSMsgType, web
from dotenv import load_dotenv

from .config import Settings
from .contracts import EventEnvelope, PingRequest, SubscriptionRequest
from .db import Database
from .events import EventBus, EventSubscription
from .runtime import HermesRuntimeAdapter
from .store import HermesStore
from .telegram_bridge import TelegramBridge


def _apply_cors(request: web.Request, response: web.StreamResponse) -> web.StreamResponse:
    origin = request.headers.get('Origin')
    if not origin:
        return response
    response.headers['Access-Control-Allow-Origin'] = origin
    response.headers['Vary'] = 'Origin'
    response.headers['Access-Control-Allow-Headers'] = 'Content-Type, Authorization'
    response.headers['Access-Control-Allow-Methods'] = 'GET, POST, OPTIONS'
    return response


def _client_ip(request: web.Request) -> str:
    peername = request.transport.get_extra_info('peername') if request.transport else None
    if isinstance(peername, tuple) and peername:
        return str(peername[0])
    return request.remote or '127.0.0.1'


@web.middleware
async def tailscale_only_middleware(request: web.Request, handler):
    app = request.app
    allowed_networks = app['allowed_networks']
    ip = ipaddress.ip_address(_client_ip(request))
    if not any(ip in network for network in allowed_networks):
        return web.json_response({'error': 'forbidden', 'message': f'client {ip} is not in the configured private allowlist'}, status=403)
    return await handler(request)


@web.middleware
async def cors_middleware(request: web.Request, handler):
    if request.method == 'OPTIONS':
        return _apply_cors(request, web.Response(status=204))
    response = await handler(request)
    return _apply_cors(request, response)


async def health(request: web.Request) -> web.Response:
    settings: Settings = request.app['settings']
    return web.json_response({'ok': True, 'telegramEnabled': settings.telegram_enabled})


async def system_status(request: web.Request) -> web.Response:
    store: HermesStore = request.app['store']
    settings: Settings = request.app['settings']
    status = store.get_system_status()
    status['connection'] = {
        'bindHost': settings.bind_host,
        'bindPort': settings.bind_port,
        'telegramEnabled': settings.telegram_enabled,
        'allowedCidrs': settings.allowed_cidrs,
    }
    return web.json_response(status)


async def list_conversations(request: web.Request) -> web.Response:
    store: HermesStore = request.app['store']
    return web.json_response([item.model_dump() for item in store.list_conversations()])


async def create_conversation(request: web.Request) -> web.Response:
    store: HermesStore = request.app['store']
    payload = await request.json() if request.can_read_body else {}
    conversation = store.create_conversation(payload.get('title'))
    return web.json_response(conversation.model_dump(), status=201)


async def get_conversation(request: web.Request) -> web.Response:
    store: HermesStore = request.app['store']
    detail = store.get_conversation_detail(request.match_info['conversation_id'])
    if not detail:
        raise web.HTTPNotFound(text='conversation not found')
    return web.json_response(detail)


async def post_message(request: web.Request) -> web.Response:
    store: HermesStore = request.app['store']
    bus: EventBus = request.app['bus']
    conversation_id = request.match_info['conversation_id']
    if not store.get_conversation(conversation_id):
        raise web.HTTPNotFound(text='conversation not found')
    payload = await request.json()
    message = store.append_message(conversation_id, 'user', payload['content'])
    event = await bus.publish(EventEnvelope(
        type='message_created', conversationId=conversation_id, runId=None,
        visibility='user', summary='User message created',
        payload={'messageId': message.id, 'role': 'user', 'content': message.content}
    ))
    return web.json_response({'message': message.model_dump(), 'event': event.model_dump()}, status=201)


async def start_run(request: web.Request) -> web.Response:
    runtime: HermesRuntimeAdapter = request.app['runtime']
    store: HermesStore = request.app['store']
    payload = await request.json()
    conversation_id = payload['conversationId']
    user_message = payload['content']
    if not store.get_conversation(conversation_id):
        raise web.HTTPNotFound(text='conversation not found')
    last = store.list_messages(conversation_id)
    if not last or last[-1].content != user_message or last[-1].role != 'user':
        store.append_message(conversation_id, 'user', user_message)
    run_id = await runtime.start_run(conversation_id, user_message)
    return web.json_response({'runId': run_id}, status=202)


async def list_runs(request: web.Request) -> web.Response:
    store: HermesStore = request.app['store']
    return web.json_response([item.model_dump() for item in store.list_runs()])


async def get_run(request: web.Request) -> web.Response:
    store: HermesStore = request.app['store']
    detail = store.get_run_detail(request.match_info['run_id'])
    if not detail:
        raise web.HTTPNotFound(text='run not found')
    return web.json_response(detail)


async def cancel_run(request: web.Request) -> web.Response:
    runtime: HermesRuntimeAdapter = request.app['runtime']
    run_id = request.match_info['run_id']
    await runtime.request_cancel(run_id)
    return web.json_response({'ok': True, 'runId': run_id})


async def retry_run(request: web.Request) -> web.Response:
    runtime: HermesRuntimeAdapter = request.app['runtime']
    run_id = request.match_info['run_id']
    new_run_id = await runtime.retry_run(run_id)
    return web.json_response({'ok': True, 'runId': new_run_id}, status=202)


async def list_events(request: web.Request) -> web.Response:
    store: HermesStore = request.app['store']
    after_seq = int(request.query.get('afterSeq', '0'))
    conversation_id = request.query.get('conversationId')
    run_id = request.query.get('runId')
    return web.json_response(store.list_events(after_seq=after_seq, conversation_id=conversation_id, run_id=run_id))


async def list_agents(request: web.Request) -> web.Response:
    store: HermesStore = request.app['store']
    active_only = request.query.get('activeOnly', '0') == '1'
    return web.json_response([item.model_dump() for item in store.list_agents(active_only=active_only)])


async def list_approvals(request: web.Request) -> web.Response:
    store: HermesStore = request.app['store']
    return web.json_response([item.model_dump() for item in store.list_pending_approvals()])


async def resolve_approval(request: web.Request) -> web.Response:
    store: HermesStore = request.app['store']
    bus: EventBus = request.app['bus']
    approval_id = request.match_info['approval_id']
    payload = await request.json()
    status = payload.get('status', 'approved')
    resolved_by = payload.get('resolvedBy', 'ghost-protocol-user')
    reason = payload.get('reason')
    record = store.resolve_approval(approval_id, status, resolved_by, reason)
    if not record:
        raise web.HTTPNotFound(text='approval not found')

    try:
        from tools.approval import resolve_gateway_approval
        resolve_gateway_approval(record.runId, 'once' if status == 'approved' else 'deny', resolve_all=False)
    except Exception:
        pass

    event = await bus.publish(EventEnvelope(
        type='approval_resolved',
        runId=record.runId,
        approvalId=record.id,
        visibility='user',
        summary='Approval resolved',
        payload={'approvalId': record.id, 'status': record.status, 'resolvedBy': resolved_by, 'reason': reason},
    ))
    return web.json_response({'approval': record.model_dump(), 'event': event.model_dump()})


async def list_artifacts(request: web.Request) -> web.Response:
    store: HermesStore = request.app['store']
    return web.json_response([item.model_dump() for item in store.list_artifacts(request.match_info['run_id'])])


async def _forward_events(ws: web.WebSocketResponse, subscription: EventSubscription) -> None:
    while True:
        event = await subscription.queue.get()
        await ws.send_json({'op': 'event', 'event': event})


async def websocket_handler(request: web.Request) -> web.StreamResponse:
    store: HermesStore = request.app['store']
    bus: EventBus = request.app['bus']
    ws = web.WebSocketResponse(heartbeat=20)
    await ws.prepare(request)
    await ws.send_json({'op': 'hello', 'message': 'Hermes Desktop daemon connected'})
    subscription = None
    forward_task: asyncio.Task | None = None
    try:
        async for msg in ws:
            if msg.type == WSMsgType.TEXT:
                payload = json.loads(msg.data)
                op = payload.get('op')
                if op == 'ping':
                    parsed = PingRequest.model_validate(payload)
                    await ws.send_json({'op': 'heartbeat', 'ts': parsed.ts})
                elif op == 'subscribe':
                    parsed = SubscriptionRequest.model_validate(payload)
                    if subscription is not None:
                        await bus.unsubscribe(subscription)
                    if forward_task is not None:
                        forward_task.cancel()
                    subscription = await bus.subscribe(parsed.conversationId, parsed.runId)
                    replay = store.list_events(after_seq=parsed.afterSeq or 0, conversation_id=parsed.conversationId, run_id=parsed.runId)
                    await ws.send_json({'op': 'subscribed', 'conversationId': parsed.conversationId, 'runId': parsed.runId, 'replayed': len(replay)})
                    for event in replay:
                        await ws.send_json({'op': 'event', 'event': event})
                    forward_task = asyncio.create_task(_forward_events(ws, subscription))
                else:
                    await ws.send_json({'op': 'error', 'message': f'unsupported op: {op}'})
            elif msg.type == WSMsgType.ERROR:
                break
    finally:
        if forward_task is not None:
            forward_task.cancel()
        if subscription is not None:
            await bus.unsubscribe(subscription)
    return ws


async def create_app() -> web.Application:
    load_dotenv()
    settings = Settings.load()
    db = Database(settings.db_path)
    store = HermesStore(db)
    bus = EventBus(store)
    runtime = HermesRuntimeAdapter(settings, store, bus)
    telegram_bridge = TelegramBridge(settings, bus, store, runtime)
    allowed_networks = [ipaddress.ip_network(item, strict=False) for item in settings.allowed_cidrs]
    app = web.Application(middlewares=[cors_middleware, tailscale_only_middleware])
    app['settings'] = settings
    app['db'] = db
    app['store'] = store
    app['bus'] = bus
    app['runtime'] = runtime
    app['telegram_bridge'] = telegram_bridge
    app['allowed_networks'] = allowed_networks

    async def on_startup(app_: web.Application) -> None:
        await app_['telegram_bridge'].start()

    async def on_cleanup(app_: web.Application) -> None:
        await app_['telegram_bridge'].stop()

    app.on_startup.append(on_startup)
    app.on_cleanup.append(on_cleanup)
    app.add_routes([
        web.get('/health', health),
        web.get('/api/system/status', system_status),
        web.get('/api/conversations', list_conversations),
        web.post('/api/conversations', create_conversation),
        web.get('/api/conversations/{conversation_id}', get_conversation),
        web.post('/api/conversations/{conversation_id}/messages', post_message),
        web.post('/api/runs', start_run),
        web.get('/api/runs', list_runs),
        web.get('/api/runs/{run_id}', get_run),
        web.post('/api/runs/{run_id}/cancel', cancel_run),
        web.post('/api/runs/{run_id}/retry', retry_run),
        web.get('/api/events', list_events),
        web.get('/api/agents', list_agents),
        web.get('/api/approvals', list_approvals),
        web.post('/api/approvals/{approval_id}/resolve', resolve_approval),
        web.get('/api/runs/{run_id}/artifacts', list_artifacts),
        web.get('/ws', websocket_handler),
    ])
    return app


def main() -> None:
    load_dotenv()
    settings = Settings.load()
    web.run_app(create_app(), host=settings.bind_host, port=settings.bind_port)


if __name__ == '__main__':
    main()
