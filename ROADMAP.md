# Roadmap

This is a directional roadmap, not a contract.

## Near-term priorities

- keep the installer and release path frictionless
- strengthen product docs and support surfaces
- keep the nostalgic TUI consistent across terminals, with docs and render tests acting as release gates
- improve reliability of sync, merge, and verify workflows
- finish the current sync parity/menu setup polish tracked in [docs/V1_3_2_PLAN.md](docs/V1_3_2_PLAN.md)

## Medium-term priorities

- deeper merge ergonomics for conflicts
- stronger import and migration paths for existing journals
- more operational guardrails around recovery and vault diagnostics
- better release automation and artifact validation

## Long-term direction

- preserve the core blue-screen journaling identity
- stay local-first
- avoid turning bsj into a general-purpose note platform
- deepen trust and durability rather than chasing feature sprawl

## v3.0 planning candidates

- local-first web app mode built around the existing encrypted vault, exposed through a local Rust service and browser UI
- Windows support planning, with web mode treated as the preferred path to cross-platform usability before any full native TUI port

See [docs/V3_0_PLAN.md](docs/V3_0_PLAN.md) for the current exploratory framing.
