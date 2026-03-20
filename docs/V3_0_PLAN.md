# BlueScreen Journal v3.0 Planning

This document tracks `v3.0` exploration themes, not release commitments.
The goal is to keep larger platform bets visible without pretending they are already scheduled.

## Planning frame

`v3.0` should only absorb work that materially expands where and how bsj can be used while preserving the local-first encrypted model.

Current planning candidates:

1. local-first web app mode
2. Windows compatibility strategy

## Candidate A: Local-first web app

Intent:

- serve a browser UI from a local Rust process on `localhost`
- reuse the existing encrypted vault, revision, search, sync, backup, and integrity logic
- preserve the nostalgic feel in a browser instead of turning bsj into a generic SaaS notes product

Why it is attractive:

- cross-platform UI is much easier in the browser than in multiple terminal stacks
- it creates a better path to Windows support
- it keeps the trust model closer to the current product than a hosted web service would

Likely scope for a first web milestone:

- unlock flow
- today editor
- index and calendar
- global search
- save, backup, sync status, verify status

Non-goal for the first pass:

- multi-tenant hosted accounts
- server-side plaintext storage
- replacing the TUI

## Candidate B: Windows support

Intent:

- make bsj usable on Windows without weakening the encrypted local-first model

Current recommendation:

- prioritize Windows support through the local web app path first
- treat a native Windows terminal port as a later decision, not the starting point

Why:

- the current app has macOS-specific assumptions around keychain, updater launch, logging paths, provider folder detection, installer flow, and soundtrack playback
- a browser UI avoids many Windows terminal compatibility problems

Likely milestones:

1. platform abstraction for key storage, logging, updater, and cloud-folder detection
2. Windows CI and packaging
3. Windows web-mode beta
4. evaluate whether a native Windows TUI is still worth building

## Entry criteria for real v3.0 execution

Before either item moves from planning to active release scope:

1. a written milestone plan exists with acceptance criteria
2. CI coverage exists for the target platform or delivery mode
3. packaging and support docs are defined up front
4. the local-first security model remains intact
