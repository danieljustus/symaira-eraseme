from symeraseme.core.db import init_db


def handle_db_init() -> str:
    path = init_db()
    return f"Database initialized at {path}"
