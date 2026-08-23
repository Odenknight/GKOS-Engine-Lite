# GKOS watcher recovery contract pack

Status: frozen Phase 5 Slice A contract/reference authority.

This pack freezes watcher delta coordination, durable observation/plan/topology authority, journal transitions, coherent activation, verified source-removal projection, status, crash recovery, and the fixed convergence sample plan. It is a contract/reference pack only; it does not activate a watcher, service, journal, provider, pointer writer, or source-removal adapter.

Phase 5 extends the engine without changing Phase 0–4 contracts or public exports. Source notes remain read-only. A physical disappearance can create an idempotent local projection-removal event, but it never asserts authored supersession, predecessor disposition, or a canonical valid_to. Phase 7 graph storage/tools are explicitly not applicable here.
