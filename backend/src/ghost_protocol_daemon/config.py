from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path


def _split_csv(value: str) -> list[str]:
    return [item.strip() for item in value.split(',') if item.strip()]


def _first_env(*names: str, default: str | None = None) -> str | None:
    for name in names:
        value = os.getenv(name)
        if value not in (None, ''):
            return value
    return default


@dataclass(slots=True)
class Settings:
    db_path: Path
    bind_host: str
    bind_port: int
    allowed_cidrs: list[str]
    hermes_source_root: Path
    model: str | None
    enabled_toolsets: list[str]
    telegram_bot_token: str | None
    telegram_chat_id: str | None
    telegram_enabled: bool

    @classmethod
    def load(cls) -> 'Settings':
        db_path = Path(_first_env('GHOST_PROTOCOL_DB', 'HERMES_DESKTOP_DB', default=str(Path.cwd() / 'data' / 'ghost_protocol.db')))
        telegram_bot_token = _first_env('GHOST_PROTOCOL_TELEGRAM_BOT_TOKEN', 'HERMES_TELEGRAM_BOT_TOKEN', 'TELEGRAM_BOT_TOKEN')
        telegram_chat_id = _first_env('GHOST_PROTOCOL_TELEGRAM_CHAT_ID', 'HERMES_TELEGRAM_CHAT_ID', 'TELEGRAM_HOME_CHANNEL')
        telegram_enabled = bool(
            telegram_bot_token
            and telegram_chat_id
            and _first_env('GHOST_PROTOCOL_TELEGRAM_ENABLED', 'HERMES_TELEGRAM_ENABLED', default='1') != '0'
        )
        return cls(
            db_path=db_path,
            bind_host=_first_env('GHOST_PROTOCOL_BIND_HOST', 'HERMES_DESKTOP_BIND_HOST', default='127.0.0.1'),
            bind_port=int(_first_env('GHOST_PROTOCOL_BIND_PORT', 'HERMES_DESKTOP_BIND_PORT', default='8787')),
            allowed_cidrs=_split_csv(_first_env('GHOST_PROTOCOL_ALLOWED_CIDRS', 'HERMES_DESKTOP_ALLOWED_CIDRS', default='100.64.0.0/10,fd7a:115c:a1e0::/48,127.0.0.1/32') or ''),
            hermes_source_root=Path(os.getenv('HERMES_SOURCE_ROOT', '/home/vmandesk/.hermes/hermes-agent')),
            model=os.getenv('HERMES_MODEL') or None,
            enabled_toolsets=_split_csv(os.getenv('HERMES_ENABLED_TOOLSETS', '')),
            telegram_bot_token=telegram_bot_token,
            telegram_chat_id=telegram_chat_id,
            telegram_enabled=telegram_enabled,
        )
