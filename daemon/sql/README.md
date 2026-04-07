This directory holds application query files.

Recommended pattern:

1. Put non-trivial queries in `daemon/sql/...`.
2. Load them in Rust with `include_str!(...)`.
3. Keep row mapping in Rust for now.
4. Validate the `.sql` files against `daemon/schema/latest.sql` with Syntaqlite.

This keeps SQL readable and toolable without forcing a full query-layer rewrite.

Suggested next step if we want more adoption:

- Migrate one store module at a time into `daemon/sql/store/<module>/...`.
