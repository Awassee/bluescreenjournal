# v2.2.4 Follow-up Plan

This follow-up patch line starts after `v2.2.3`.

The goal is to keep the stable release line healthy without changing the core writing surface unnecessarily.

## Candidate follow-up work

1. Keep installer and release validation resilient under low-disk conditions.
   - Acceptance:
   - release smoke failures distinguish product bugs from disk-pressure failures clearly
   - source-update validation can be rerun cleanly after a standard cleanup step

2. Improve conflict merge ergonomics.
   - Acceptance:
   - conflicted dates are easier to read before entering merge
   - merge choices explain which revision wins and why

3. Strengthen restore and recovery drills in-product.
   - Acceptance:
   - restore previews are easier to understand
   - recovery paths feel rehearsed, not theoretical

4. Improve cloud sync observability.
   - Acceptance:
   - active backend, remote target, and last successful sync are visible at a glance
   - users can tell whether recovery is possible before they need it

5. Keep release confidence high.
   - Acceptance:
   - installer regression coverage stays current
   - certification and maintenance docs stay aligned with the current stable line
