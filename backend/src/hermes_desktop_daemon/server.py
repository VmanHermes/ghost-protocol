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


async def health(_: web.Request) -> web.Response:
    return web.json_response({'ok': True})


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
    conversation = store.get_conversation(request.match_info['conversation_id'])
    if not conversation:
        raise web.HTTPNotFound(text='conversation not found')
    messages = [item.model_dump() for item in store.list_messages(conversation.id)]
    return web.json_response({'conversation': conversation.model_dump(), 'messages': messages})


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
    return web.json_response([item.model_dump() for item in store.list_runs() if item])


async def get_run(request: web.Request) -> web.Response:
    store: HermesStore = request.app['store']
    run_id = request.match_info['run_id']
    run = store.get_run(run_id)
    if not run:
        raise web.HTTPNotFound(text='run not found')
    return web.json_response({
        'run': run.model_dump(),
        'live': store.get_run_live(run_id),
        'timeline': store.list_run_timeline(run_id),
    })


async def list_events(request: web.Request) -> web.Response:
    store: HermesStore = request.app['store']
    after_seq = int(request.query.get('afterSeq', '0'))
    conversation_id = request.query.get('conversationId')
    run_id = request.query.get('runId')
    return web.json_response(store.list_events(after_seq=after_seq, conversation_id=conversation_id, run_id=run_id))


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


async def list_agents(_: web.Request) -> web.Response:
    return web.json_response([])


async def list_approvals(_: web.Request) -> web.Response:
    return web.json_response([])


async def list_artifacts(_: web.Request) -> web.Response:
    return web.json_response([])


async def create_app() -> web.Application:
    load_dotenv()
    settings = Settings.load()
    db = Database(settings.db_path)
    store = HermesStore(db)
    bus = EventBus(store)
    runtime = HermesRuntimeAdapter(settings, store, bus)
    allowed_networks = [ipaddress.ip_network(item, strict=False) for item in settings.allowed_cidrs]
    app = web.Application(middlewares=[tailscale_only_middleware])
    app['settings'] = settings
    app['db'] = db
    app['store'] = store
    app['bus'] = bus
    app['runtime'] = runtime
    app['allowed_networks'] = allowed_networks
    app.add_routes([
        web.get('/health', health),
        web.get('/api/conversations', list_conversations),
        web.post('/api/conversations', create_conversation),
        web.get('/api/conversations/{conversation_id}', get_conversation),
        web.post('/api/conversations/{conversation_id}/messages', post_message),
        web.post('/api/runs', start_run),
        web.get('/api/runs', list_runs),
        web.get('/api/runs/{run_id}', get_run),
        web.get('/api/events', list_events),
        web.get('/api/agents', list_agents),
        web.get('/api/approvals', list_approvals),
        web.get('/api/runs/{run_id}/artifacts', list_artifacts),
        web.get('/ws', websocket_handler),
    ])
    return app


def main() -> None:
    settings = Settings.load()
    web.run_app(create_app(), host=settings.bind_host, port=settings.bind_port)


if __name__ == '__main__':
    main()
