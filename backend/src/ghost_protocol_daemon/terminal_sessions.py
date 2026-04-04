from __future__ import annotations

import asyncio
import logging
import os
import pty
import signal
import struct
import subprocess
import termios
import threading
from dataclasses import dataclass
from pathlib import Path

from .contracts import TerminalSessionMode, TerminalSessionRecord
from .store import HermesStore, now_iso

log = logging.getLogger('ghost_protocol.terminal')


@dataclass(eq=False, slots=True)
class TerminalSessionSubscription:
    queue: asyncio.Queue
    session_id: str

    def matches(self, session_id: str) -> bool:
        return self.session_id == session_id


@dataclass(slots=True)
class ManagedTerminalSession:
    session_id: str
    process: subprocess.Popen[bytes]
    master_fd: int
    tmux_session_name: str


class RemoteSessionManager:
    IDLE_TIMEOUT_SECONDS: float = 120  # kill tmux session 2 min after last subscriber leaves

    def __init__(self, settings, store: HermesStore):
        self.settings = settings
        self.store = store
        self._sessions: dict[str, ManagedTerminalSession] = {}
        self._subscriptions: set[TerminalSessionSubscription] = set()
        self._lock = asyncio.Lock()
        self._loop: asyncio.AbstractEventLoop | None = None
        self._idle_timers: dict[str, asyncio.TimerHandle] = {}

    def bind_loop(self, loop: asyncio.AbstractEventLoop) -> None:
        self._loop = loop

    # ------------------------------------------------------------------
    # Recovery
    # ------------------------------------------------------------------

    def recover_existing_sessions(self) -> None:
        # Verify tmux is available
        check = subprocess.run(['tmux', '-V'], capture_output=True, text=True, check=False)
        if check.returncode != 0:
            log.error('tmux is not installed or not in PATH — terminal sessions will not work')
            # Still mark stale DB records as terminated
            for session in self.store.list_terminal_sessions():
                if session.status in {'created', 'running'}:
                    self.store.update_terminal_session(session.id, status='terminated', finishedAt=now_iso())
            return

        active_tmux = set(self._list_tmux_sessions())
        ghost_tmux = {name for name in active_tmux if name.startswith('ghost-')}
        log.info('Recovery: found %d tmux sessions (%d ghost-prefixed)', len(active_tmux), len(ghost_tmux))

        claimed: set[str] = set()

        for session in self.store.list_terminal_sessions():
            if session.status not in {'created', 'running'}:
                continue
            session_name = self._tmux_session_name(session.id)

            if session_name in ghost_tmux:
                # tmux session survived daemon restart — keep alive, lazy reattach on subscribe
                log.info('Recovery: session %s (%s) alive in tmux, keeping', session.id[:8], session_name)
                self.store.update_terminal_session(session.id, status='running', pid=None)
                claimed.add(session_name)
            else:
                log.info('Recovery: session %s (%s) tmux gone, marking terminated', session.id[:8], session_name)
                self.store.update_terminal_session(session.id, status='terminated', finishedAt=now_iso())

        # Kill orphaned tmux sessions (ghost- prefix but no DB record)
        for orphan in ghost_tmux - claimed:
            log.info('Recovery: killing orphaned tmux session %s', orphan)
            subprocess.run(
                ['tmux', 'kill-session', '-t', orphan],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False,
            )

    # ------------------------------------------------------------------
    # Subscriptions
    # ------------------------------------------------------------------

    async def subscribe(self, session_id: str) -> TerminalSessionSubscription:
        subscription = TerminalSessionSubscription(queue=asyncio.Queue(), session_id=session_id)
        async with self._lock:
            self._subscriptions.add(subscription)
            # Cancel any pending idle termination — someone reconnected
            timer = self._idle_timers.pop(session_id, None)
            if timer is not None:
                timer.cancel()
        return subscription

    async def unsubscribe(self, subscription: TerminalSessionSubscription) -> None:
        async with self._lock:
            self._subscriptions.discard(subscription)
            has_subscribers = any(s.session_id == subscription.session_id for s in self._subscriptions)
        if not has_subscribers:
            session_id = subscription.session_id
            log.info('Session %s: last subscriber left, detaching', session_id[:8])
            self._detach_session(session_id)
            # Schedule tmux session termination after grace period
            if self._loop is not None:
                self._idle_timers[session_id] = self._loop.call_later(
                    self.IDLE_TIMEOUT_SECONDS,
                    lambda sid=session_id: asyncio.ensure_future(self._idle_terminate(sid)),
                )

    # ------------------------------------------------------------------
    # Session lifecycle
    # ------------------------------------------------------------------

    async def create_session(
        self,
        *,
        mode: TerminalSessionMode = 'agent',
        name: str | None = None,
        workdir: str | None = None,
    ) -> TerminalSessionRecord:
        session_workdir = self._resolve_workdir(workdir)
        record = self.store.create_terminal_session(
            mode=mode,
            name=name,
            workdir=session_workdir,
            command=['tmux', 'new-session', '-d'],  # placeholder, updated below
        )
        managed = await asyncio.to_thread(self._spawn_tmux_attach, record, True)
        self._sessions[record.id] = managed
        self.store.update_terminal_session(
            record.id,
            status='running',
            startedAt=now_iso(),
            pid=managed.process.pid,
            command=self._attach_command(record.id),
        )
        self._start_stream_threads(record.id)
        updated = self.store.get_terminal_session(record.id)
        if updated is None:
            raise RuntimeError('failed to load created terminal session')
        log.info('Session %s: created (%s) pid=%s', record.id[:8], mode, managed.process.pid)
        await self._broadcast(record.id, {'op': 'terminal_status', 'session': updated.model_dump()})
        return updated

    async def ensure_session_attached(self, session_id: str) -> TerminalSessionRecord:
        record = self.store.get_terminal_session(session_id)
        if record is None:
            raise ValueError('terminal session not found')
        if session_id in self._sessions:
            return record
        if not await asyncio.to_thread(self._tmux_session_exists, session_id):
            self.store.update_terminal_session(session_id, status='terminated', finishedAt=now_iso())
            updated = self.store.get_terminal_session(session_id)
            if updated is None:
                raise ValueError('terminal session not found')
            return updated
        managed = await asyncio.to_thread(self._spawn_tmux_attach, record, False)
        self._sessions[session_id] = managed
        self.store.update_terminal_session(session_id, status='running', pid=managed.process.pid, command=self._attach_command(session_id))
        self._start_stream_threads(session_id)
        updated = self.store.get_terminal_session(session_id)
        if updated is None:
            raise ValueError('terminal session not found')
        return updated

    async def send_input(self, session_id: str, data: str, *, append_newline: bool = True) -> None:
        managed = self._sessions.get(session_id)
        if managed is not None:
            # Hot path: write directly to the PTY master fd — no subprocess, no DB lookup
            raw = data if not append_newline else data + '\n'
            if raw:
                os.write(managed.master_fd, raw.encode())
            return
        # Cold path: reattach, then write
        await self.ensure_session_attached(session_id)
        managed = self._sessions.get(session_id)
        if managed is not None:
            raw = data if not append_newline else data + '\n'
            if raw:
                os.write(managed.master_fd, raw.encode())

    async def resize_session(self, session_id: str, *, cols: int, rows: int) -> TerminalSessionRecord:
        record = await self.ensure_session_attached(session_id)
        managed = self._sessions.get(session_id)
        if managed is not None:
            # Resize PTY first (fast, synchronous ioctl)
            self._set_window_size(managed.master_fd, cols, rows)
            # Then tell tmux to resize its window to match — run in background, don't block
            asyncio.to_thread(
                subprocess.run,
                ['tmux', 'resize-window', '-t', self._tmux_session_name(session_id), '-x', str(cols), '-y', str(rows)],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False,
            )
        return record

    async def terminate_session(self, session_id: str) -> TerminalSessionRecord:
        timer = self._idle_timers.pop(session_id, None)
        if timer is not None:
            timer.cancel()
        await asyncio.to_thread(self._kill_tmux_session, session_id)
        managed = self._sessions.pop(session_id, None)
        if managed is not None:
            self._terminate_attach_process(managed)
        self.store.update_terminal_session(session_id, status='terminated', finishedAt=now_iso(), pid=None)
        record = self.store.get_terminal_session(session_id)
        if record is None:
            raise ValueError('terminal session not found')
        await self._broadcast(session_id, {'op': 'terminal_status', 'session': record.model_dump()})
        return record

    # ------------------------------------------------------------------
    # Streaming threads
    # ------------------------------------------------------------------

    def _start_stream_threads(self, session_id: str) -> None:
        threading.Thread(target=self._reader_thread, args=(session_id,), daemon=True).start()
        threading.Thread(target=self._waiter_thread, args=(session_id,), daemon=True).start()

    def _reader_thread(self, session_id: str) -> None:
        session = self._sessions.get(session_id)
        if session is None:
            return
        while True:
            try:
                chunk = os.read(session.master_fd, 16384)  # 16 KiB reads for lower latency
            except OSError:
                break
            if not chunk:
                break
            if self._loop is None:
                continue
            text = chunk.decode(errors='replace')
            asyncio.run_coroutine_threadsafe(self._publish_chunk(session_id, text), self._loop)

    def _waiter_thread(self, session_id: str) -> None:
        session = self._sessions.get(session_id)
        if session is None:
            return
        exit_code = session.process.wait()
        if self._loop is None:
            return
        asyncio.run_coroutine_threadsafe(self._handle_attach_exit(session_id, exit_code), self._loop)

    async def _publish_chunk(self, session_id: str, chunk: str) -> None:
        # Persist first to get the canonical DB-assigned chunk ID.
        # The frontend uses this ID for dedup and replay continuity,
        # so broadcast and replay MUST use the same ID space.
        record = await asyncio.to_thread(self.store.append_terminal_chunk, session_id, 'stdout', chunk)
        await self._broadcast(session_id, {'op': 'terminal_chunk', 'chunk': {
            'id': record.id,
            'sessionId': session_id,
            'stream': 'stdout',
            'chunk': chunk,
            'createdAt': record.createdAt,
        }})

    async def _handle_attach_exit(self, session_id: str, exit_code: int) -> None:
        managed = self._sessions.pop(session_id, None)
        if managed is not None:
            try:
                os.close(managed.master_fd)
            except OSError:
                pass
        session_exists = await asyncio.to_thread(self._tmux_session_exists, session_id)
        if session_exists:
            # tmux session still alive — attach process exited but shell continues
            self.store.update_terminal_session(session_id, status='running', pid=None, command=self._attach_command(session_id))
        else:
            current = self.store.get_terminal_session(session_id)
            if current is None:
                return
            status = current.status
            if status != 'terminated':
                status = 'exited' if exit_code == 0 else 'error'
            self.store.update_terminal_session(session_id, status=status, exitCode=exit_code, finishedAt=now_iso(), pid=None)
        updated = self.store.get_terminal_session(session_id)
        if updated is not None:
            await self._broadcast(session_id, {'op': 'terminal_status', 'session': updated.model_dump()})

    def _detach_session(self, session_id: str) -> None:
        """Close the daemon-side attach process (PTY fd + threads) without killing the tmux session."""
        managed = self._sessions.pop(session_id, None)
        if managed is not None:
            self._terminate_attach_process(managed)

    async def _idle_terminate(self, session_id: str) -> None:
        """Terminate a tmux session after it has been idle (no subscribers) past the grace period."""
        self._idle_timers.pop(session_id, None)
        async with self._lock:
            has_subscribers = any(s.session_id == session_id for s in self._subscriptions)
        if has_subscribers:
            return
        log.info('Session %s: idle timeout expired, killing tmux session', session_id[:8])
        await asyncio.to_thread(self._kill_tmux_session, session_id)
        self._sessions.pop(session_id, None)
        self.store.update_terminal_session(session_id, status='terminated', finishedAt=now_iso(), pid=None)
        record = self.store.get_terminal_session(session_id)
        if record is not None:
            await self._broadcast(session_id, {'op': 'terminal_status', 'session': record.model_dump()})

    async def _broadcast(self, session_id: str, message: dict) -> None:
        async with self._lock:
            subscriptions = list(self._subscriptions)
        for subscription in subscriptions:
            if subscription.matches(session_id):
                await subscription.queue.put(message)

    # ------------------------------------------------------------------
    # tmux process management
    # ------------------------------------------------------------------

    def _create_tmux_session(
        self,
        session_id: str,
        *,
        workdir: str,
        cols: int = 120,
        rows: int = 32,
        mode: TerminalSessionMode | None = None,
    ) -> None:
        """Create a detached tmux session. The shell starts immediately inside tmux."""
        session_name = self._tmux_session_name(session_id)
        shell = self._default_shell(mode)

        cmd = [
            'tmux', 'new-session',
            '-d',                    # detached — no client attached yet
            '-s', session_name,
            '-x', str(cols),
            '-y', str(rows),
            '-c', workdir,
            shell,                   # command to run (executed via default shell)
        ]
        result = subprocess.run(cmd, capture_output=True, text=True, check=False)
        if result.returncode != 0:
            raise RuntimeError(f'tmux new-session failed: {result.stderr.strip()}')

        # Configure tmux for transparency — suppress all UI chrome.
        # Batch all set-option calls into one subprocess for speed.
        subprocess.run(
            [
                'tmux',
                'set-option', '-t', session_name, 'status', 'off', ';',
                'set-option', '-t', session_name, 'pane-border-status', 'off', ';',
                'set-option', '-t', session_name, 'mouse', 'off',
            ],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False,
        )

    def _spawn_tmux_attach(self, record: TerminalSessionRecord, create: bool) -> ManagedTerminalSession:
        """Spawn a tmux attach process connected to our PTY pair for streaming I/O."""
        session_name = self._tmux_session_name(record.id)

        if create:
            self._create_tmux_session(
                record.id,
                workdir=record.workdir,
                mode=record.mode,
            )

        master_fd, slave_fd = pty.openpty()
        self._set_window_size(master_fd, cols=120, rows=32)

        env = os.environ.copy()
        env.setdefault('TERM', 'xterm-256color')
        env.setdefault('COLORTERM', 'truecolor')
        env.setdefault('TERM_PROGRAM', 'Ghost Protocol')
        env['GHOST_PROTOCOL_REMOTE_SESSION'] = '1'

        command = self._attach_command(record.id)
        process = subprocess.Popen(
            command,
            cwd=record.workdir,
            env=env,
            stdin=slave_fd,
            stdout=slave_fd,
            stderr=slave_fd,
            start_new_session=True,
            close_fds=True,
        )
        os.close(slave_fd)

        return ManagedTerminalSession(
            session_id=record.id,
            process=process,
            master_fd=master_fd,
            tmux_session_name=session_name,
        )

    def _attach_command(self, session_id: str) -> list[str]:
        return ['tmux', 'attach-session', '-t', self._tmux_session_name(session_id)]

    def _default_shell(self, mode: TerminalSessionMode | None) -> str:
        if mode in {'agent', 'project'}:
            return 'hermes chat'
        return os.getenv('SHELL', '/bin/bash')

    # ------------------------------------------------------------------
    # tmux helpers (all synchronous, called via asyncio.to_thread)
    # ------------------------------------------------------------------

    def _tmux_session_name(self, session_id: str) -> str:
        return f'ghost-{session_id.replace("-", "")}'

    def _tmux_session_exists(self, session_id: str) -> bool:
        """O(1) check using tmux has-session instead of listing all sessions."""
        return subprocess.run(
            ['tmux', 'has-session', '-t', self._tmux_session_name(session_id)],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False,
        ).returncode == 0

    def _list_tmux_sessions(self) -> list[str]:
        result = subprocess.run(
            ['tmux', 'list-sessions', '-F', '#{session_name}'],
            capture_output=True, text=True, check=False,
        )
        if result.returncode != 0:
            return []
        return [line.strip() for line in result.stdout.splitlines() if line.strip()]

    def _kill_tmux_session(self, session_id: str) -> None:
        subprocess.run(
            ['tmux', 'kill-session', '-t', self._tmux_session_name(session_id)],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False,
        )

    # ------------------------------------------------------------------
    # Process and PTY utilities
    # ------------------------------------------------------------------

    def _terminate_attach_process(self, managed: ManagedTerminalSession) -> None:
        try:
            os.killpg(os.getpgid(managed.process.pid), signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            os.close(managed.master_fd)
        except OSError:
            pass

    def _resolve_workdir(self, workdir: str | None) -> str:
        if workdir:
            return str(Path(workdir).expanduser())
        return str(Path.home())

    @staticmethod
    def _set_window_size(master_fd: int, cols: int, rows: int) -> None:
        winsize = struct.pack('HHHH', rows, cols, 0, 0)
        termios.tcsetwinsize(master_fd, (rows, cols))
        try:
            import fcntl
            fcntl.ioctl(master_fd, termios.TIOCSWINSZ, winsize)
        except OSError:
            pass
