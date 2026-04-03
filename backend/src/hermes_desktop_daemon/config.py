from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path


def _split_csv(value: str) -> list[str]:
    return [item.strip() for item in value.split(',') if item.strip()]


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
        db_path = Path(os.getenv('HERMES_DESKTOP_DB', str(Path.cwd() / 'data' / 'hermes_desktop.db')))
        telegram_bot_token = os.getenv('HERMES_TELEGRAM_BOT_TOKEN') or os.getenv('TELEGRAM_BOT_TOKEN') or None
        telegram_chat_id = os.getenv('HERMES_TELEGRAM_CHAT_ID') or os.getenv('TELEGRAM_HOME_CHANNEL') or None
        telegram_enabled = bool(telegram_bot_token and telegram_chat_id and os.getenv('HERMES_TELEGRAM_ENABLED', '1') != '0')
        return cls(
            db_path=db_path,
            bind_host=os.getenv('HERMES_DESKTOP_BIND_HOST', '127.0.0.1'),
            bind_port=int(os.getenv('HERMES_DESKTOP_BIND_PORT', '8787')),
            allowed_cidrs=_split_csv(os.getenv('HERMES_DESKTOP_ALLOWED_CIDRS', '100.64.0.0/10,fd7a:115c:a1e0::/48,127.0.0.1/32')),
            hermes_source_root=Path(os.getenv('HERMES_SOURCE_ROOT', '/home/vmandesk/.hermes/hermes-agent')),
            model=os.getenv('HERMES_MODEL') or None,
            enabled_toolsets=_split_csv(os.getenv('HERMES_ENABLED_TOOLSETS', '')),
            telegram_bot_token=telegram_bot_token,
            telegram_chat_id=telegram_chat_id,
            telegram_enabled=telegram_enabled,
        )
