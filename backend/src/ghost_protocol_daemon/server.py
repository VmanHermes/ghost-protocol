from __future__ import annotations

import asyncio
import ipaddress
import json
import logging
import logging.handlers
from collections import deque

from aiohttp import WSMsgType, web
from dotenv import load_dotenv

from .config import Settings
from .contracts import EventEnvelope, PingRequest, SubscriptionRequest, TerminalSubscriptionRequest
from .db import Database
from .events import EventBus, EventSubscription
from .runtime import HermesRuntimeAdapter
from .store import HermesStore, now_iso
from .telegram_bridge import TelegramBridge
from .terminal_sessions import RemoteSessionManager, TerminalSessionSubscription

log = logging.getLogger('ghost_protocol')


class InMemoryLogHandler(logging.Handler):
    """Ring buffer handler that keeps the last N log records for the /api/system/logs endpoint."""

    def __init__(self, capacity: int = 500):
        super().__init__()
        self._buffer: deque[dict] = deque(maxlen=capacity)

    def emit(self, record: logging.LogRecord) -> None:
        self._buffer.append({
            'ts': now_iso(),
            'level': record.levelname,
            'logger': record.name,
            'message': self.format(record),
        })

    def entries(self, limit: int = 200, level: str | None = None) -> list[dict]:
        items = list(self._buffer)
        if level:
            items = [e for e in items if e['level'] == level.upper()]
        return items[-limit:]


_memory_handler = InMemoryLogHandler(capacity=1000)


def setup_logging(settings: Settings) -> None:
    settings.log_dir.mkdir(parents=True, exist_ok=True)
    log_file = settings.log_dir / 'daemon.log'

    root = logging.getLogger()
    root.setLevel(logging.DEBUG)

    fmt = logging.Formatter('%(asctime)s %(levelname)-5s [%(name)s] %(message)s', datefmt='%Y-%m-%d %H:%M:%S')

    # File handler — rotates at 5 MB, keeps 3 backups
    fh = logging.handlers.RotatingFileHandler(log_file, maxBytes=5 * 1024 * 1024, backupCount=3)
    fh.setLevel(logging.DEBUG)
    fh.setFormatter(fmt)
    root.addHandler(fh)

    # Stderr handler
    sh = logging.StreamHandler()
    sh.setLevel(logging.INFO)
    sh.setFormatter(fmt)
    root.addHandler(sh)

    # In-memory handler for API
    _memory_handler.setLevel(logging.DEBUG)
    _memory_handler.setFormatter(fmt)
    root.addHandler(_memory_handler)

    # Quiet noisy libraries
    logging.getLogger('aiohttp').setLevel(logging.WARNING)
    logging.getLogger('asyncio').setLevel(logging.WARNING)


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
    try:
        response = await handler(request)
    except web.HTTPException as exc:
        response = exc
    except Exception as exc:
        log.exception('Unhandled error in %s %s', request.method, request.path)
        response = web.json_response({'error': 'internal_error', 'message': str(exc)}, status=500)
    return _apply_cors(request, response)


async def health(request: web.Request) -> web.Response:
    settings: Settings = request.app['settings']
    return web.json_response({'ok': True, 'telegramEnabled': settings.telegram_enabled})


async def system_logs(request: web.Request) -> web.Response:
    limit = int(request.query.get('limit', '200'))
    level = request.query.get('level')
    return web.json_response(_memory_handler.entries(limit=limit, level=level))


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


async def _forward_terminal_messages(ws: web.WebSocketResponse, subscription: TerminalSessionSubscription) -> None:
    while True:
        message = await subscription.queue.get()
        await ws.send_json(message)


async def list_terminal_sessions(request: web.Request) -> web.Response:
    store: HermesStore = request.app['store']
    return web.json_response([item.model_dump() for item in store.list_terminal_sessions()])


async def create_terminal_session(request: web.Request) -> web.Response:
    manager: RemoteSessionManager = request.app['remote_sessions']
    payload = await request.json() if request.can_read_body else {}
    session = await manager.create_session(
        mode=payload.get('mode', 'agent'),
        name=payload.get('name'),
        workdir=payload.get('workdir'),
    )
    return web.json_response(session.model_dump(), status=201)


async def get_terminal_session(request: web.Request) -> web.Response:
    manager: RemoteSessionManager = request.app['remote_sessions']
    store: HermesStore = request.app['store']
    await manager.ensure_session_attached(request.match_info['session_id'])
    detail = store.get_terminal_session_detail(request.match_info['session_id'])
    if not detail:
        raise web.HTTPNotFound(text='terminal session not found')
    return web.json_response(detail)


async def post_terminal_input(request: web.Request) -> web.Response:
    manager: RemoteSessionManager = request.app['remote_sessions']
    payload = await request.json()
    await manager.send_input(
        request.match_info['session_id'],
        payload.get('input', ''),
        append_newline=payload.get('appendNewline', True),
    )
    return web.json_response({'ok': True})


async def resize_terminal_session(request: web.Request) -> web.Response:
    manager: RemoteSessionManager = request.app['remote_sessions']
    payload = await request.json()
    session = await manager.resize_session(
        request.match_info['session_id'],
        cols=int(payload.get('cols', 120)),
        rows=int(payload.get('rows', 32)),
    )
    return web.json_response(session.model_dump())


async def terminate_terminal_session(request: web.Request) -> web.Response:
    manager: RemoteSessionManager = request.app['remote_sessions']
    session = await manager.terminate_session(request.match_info['session_id'])
    return web.json_response(session.model_dump())


async def websocket_handler(request: web.Request) -> web.StreamResponse:
    client_ip = _client_ip(request)
    store: HermesStore = request.app['store']
    bus: EventBus = request.app['bus']
    remote_sessions: RemoteSessionManager = request.app['remote_sessions']
    ws = web.WebSocketResponse(heartbeat=20)
    await ws.prepare(request)
    log.info('WS connected from %s', client_ip)
    await ws.send_json({'op': 'hello', 'message': 'Ghost Protocol daemon connected'})
    subscription = None
    terminal_subscription = None
    forward_task: asyncio.Task | None = None
    try:
        async for msg in ws:
            if msg.type == WSMsgType.TEXT:
                try:
                    payload = json.loads(msg.data)
                except json.JSONDecodeError:
                    log.warning('WS %s: invalid JSON received', client_ip)
                    await ws.send_json({'op': 'error', 'message': 'invalid JSON'})
                    continue
                op = payload.get('op')
                if op == 'ping':
                    parsed = PingRequest.model_validate(payload)
                    await ws.send_json({'op': 'heartbeat', 'ts': parsed.ts})
                elif op == 'subscribe':
                    parsed = SubscriptionRequest.model_validate(payload)
                    if subscription is not None:
                        await bus.unsubscribe(subscription)
                    if terminal_subscription is not None:
                        await remote_sessions.unsubscribe(terminal_subscription)
                        terminal_subscription = None
                    if forward_task is not None:
                        forward_task.cancel()
                    subscription = await bus.subscribe(parsed.conversationId, parsed.runId)
                    replay = store.list_events(after_seq=parsed.afterSeq or 0, conversation_id=parsed.conversationId, run_id=parsed.runId)
                    await ws.send_json({'op': 'subscribed', 'conversationId': parsed.conversationId, 'runId': parsed.runId, 'replayed': len(replay)})
                    for event in replay:
                        await ws.send_json({'op': 'event', 'event': event})
                    forward_task = asyncio.create_task(_forward_events(ws, subscription))
                elif op == 'subscribe_terminal':
                    parsed = TerminalSubscriptionRequest.model_validate(payload)
                    log.info('WS %s: subscribe_terminal session=%s', client_ip, parsed.sessionId[:8])
                    if subscription is not None:
                        await bus.unsubscribe(subscription)
                        subscription = None
                    if terminal_subscription is not None:
                        await remote_sessions.unsubscribe(terminal_subscription)
                    if forward_task is not None:
                        forward_task.cancel()
                    try:
                        await remote_sessions.ensure_session_attached(parsed.sessionId)
                    except Exception:
                        log.exception('WS %s: failed to attach session %s', client_ip, parsed.sessionId[:8])
                        await ws.send_json({'op': 'error', 'message': f'failed to attach terminal session {parsed.sessionId[:8]}'})
                        continue
                    terminal_subscription = await remote_sessions.subscribe(parsed.sessionId)
                    replay = [item.model_dump() for item in store.list_terminal_chunks(parsed.sessionId, after_chunk_id=parsed.afterChunkId or 0)]
                    detail = store.get_terminal_session_detail(parsed.sessionId)
                    log.info('WS %s: terminal subscribed, replaying %d chunks', client_ip, len(replay))
                    await ws.send_json({'op': 'subscribed_terminal', 'sessionId': parsed.sessionId, 'replayed': len(replay), 'session': detail['session'] if detail else None})
                    for chunk in replay:
                        await ws.send_json({'op': 'terminal_chunk', 'chunk': chunk})
                    forward_task = asyncio.create_task(_forward_terminal_messages(ws, terminal_subscription))
                elif op == 'terminal_input':
                    session_id = payload.get('sessionId')
                    if not session_id:
                        await ws.send_json({'op': 'error', 'message': 'sessionId is required for terminal_input'})
                        continue
                    await remote_sessions.send_input(
                        session_id,
                        payload.get('input', ''),
                        append_newline=bool(payload.get('appendNewline', False)),
                    )
                elif op == 'resize_terminal':
                    session_id = payload.get('sessionId')
                    if not session_id:
                        await ws.send_json({'op': 'error', 'message': 'sessionId is required for resize_terminal'})
                        continue
                    session = await remote_sessions.resize_session(
                        session_id,
                        cols=int(payload.get('cols', 120)),
                        rows=int(payload.get('rows', 32)),
                    )
                    await ws.send_json({'op': 'terminal_status', 'session': session.model_dump()})
                elif op == 'interrupt_terminal':
                    session_id = payload.get('sessionId')
                    if not session_id:
                        await ws.send_json({'op': 'error', 'message': 'sessionId is required for interrupt_terminal'})
                        continue
                    await remote_sessions.send_input(session_id, '\u0003', append_newline=False)
                elif op == 'terminate_terminal':
                    session_id = payload.get('sessionId')
                    if not session_id:
                        await ws.send_json({'op': 'error', 'message': 'sessionId is required for terminate_terminal'})
                        continue
                    session = await remote_sessions.terminate_session(session_id)
                    await ws.send_json({'op': 'terminal_status', 'session': session.model_dump()})
                else:
                    await ws.send_json({'op': 'error', 'message': f'unsupported op: {op}'})
            elif msg.type == WSMsgType.ERROR:
                log.warning('WS %s: received error frame', client_ip)
                break
    except Exception:
        log.exception('WS %s: unhandled error in message loop', client_ip)
    finally:
        log.info('WS %s: disconnected', client_ip)
        if forward_task is not None:
            forward_task.cancel()
        if subscription is not None:
            await bus.unsubscribe(subscription)
        if terminal_subscription is not None:
            await remote_sessions.unsubscribe(terminal_subscription)
    return ws


async def create_app() -> web.Application:
    load_dotenv()
    settings = Settings.load()
    db = Database(settings.db_path)
    store = HermesStore(db)
    bus = EventBus(store)
    runtime = HermesRuntimeAdapter(settings, store, bus)
    remote_sessions = RemoteSessionManager(settings, store)
    telegram_bridge = TelegramBridge(settings, bus, store, runtime)
    allowed_networks = [ipaddress.ip_network(item, strict=False) for item in settings.allowed_cidrs]
    app = web.Application(middlewares=[cors_middleware, tailscale_only_middleware])
    app['settings'] = settings
    app['db'] = db
    app['store'] = store
    app['bus'] = bus
    app['runtime'] = runtime
    app['remote_sessions'] = remote_sessions
    app['telegram_bridge'] = telegram_bridge
    app['allowed_networks'] = allowed_networks

    async def on_startup(app_: web.Application) -> None:
        log.info('Daemon startup: recovering sessions, starting bridges')
        app_['remote_sessions'].bind_loop(asyncio.get_running_loop())
        app_['remote_sessions'].recover_existing_sessions()
        await app_['telegram_bridge'].start()
        log.info('Daemon startup complete')

    async def on_cleanup(app_: web.Application) -> None:
        await app_['telegram_bridge'].stop()

    app.on_startup.append(on_startup)
    app.on_cleanup.append(on_cleanup)
    app.add_routes([
        web.get('/health', health),
        web.get('/api/system/status', system_status),
        web.get('/api/system/logs', system_logs),
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
        web.get('/api/terminal/sessions', list_terminal_sessions),
        web.post('/api/terminal/sessions', create_terminal_session),
        web.get('/api/terminal/sessions/{session_id}', get_terminal_session),
        web.post('/api/terminal/sessions/{session_id}/input', post_terminal_input),
        web.post('/api/terminal/sessions/{session_id}/resize', resize_terminal_session),
        web.post('/api/terminal/sessions/{session_id}/terminate', terminate_terminal_session),
        web.get('/ws', websocket_handler),
    ])
    return app


def main() -> None:
    load_dotenv()
    settings = Settings.load()
    setup_logging(settings)
    log.info('Ghost Protocol daemon starting on %s:%s', settings.bind_host, settings.bind_port)
    web.run_app(create_app(), host=settings.bind_host, port=settings.bind_port)


if __name__ == '__main__':
    main()
