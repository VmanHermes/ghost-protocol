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

    @classmethod
    def load(cls) -> 'Settings':
        db_path = Path(os.getenv('HERMES_DESKTOP_DB', str(Path.cwd() / 'data' / 'hermes_desktop.db')))
        return cls(
            db_path=db_path,
            bind_host=os.getenv('HERMES_DESKTOP_BIND_HOST', '127.0.0.1'),
            bind_port=int(os.getenv('HERMES_DESKTOP_BIND_PORT', '8787')),
            allowed_cidrs=_split_csv(os.getenv('HERMES_DESKTOP_ALLOWED_CIDRS', '100.64.0.0/10,fd7a:115c:a1e0::/48,127.0.0.1/32')),
            hermes_source_root=Path(os.getenv('HERMES_SOURCE_ROOT', '/home/vmandesk/.hermes/hermes-agent')),
            model=os.getenv('HERMES_MODEL') or None,
            enabled_toolsets=_split_csv(os.getenv('HERMES_ENABLED_TOOLSETS', '')),
        )
