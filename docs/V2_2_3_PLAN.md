# v2.2.3 Follow-up Plan

This follow-up patch line starts after `v2.2.2`.

The goal is to keep the release line healthy without destabilizing the core writing surface.

## Candidate follow-up work

1. Improve conflict merge ergonomics.
   - Acceptance:
   - conflicted dates are easier to read before entering merge
   - merge choices explain which revision wins and why

2. Strengthen restore and recovery drills in-product.
   - Acceptance:
   - restore previews are easier to understand
   - recovery paths feel rehearsed, not theoretical

3. Improve cloud sync observability.
   - Acceptance:
   - active backend, remote target, and last successful sync are visible at a glance
   - users can tell whether recovery is possible before they need it

4. Deepen spellcheck into a better writing assistant.
   - Acceptance:
   - session dictionary and personal dictionary flows feel clearer
   - typo help stays menu-first and low-friction

5. Keep release confidence high.
   - Acceptance:
   - installer regression coverage stays current
   - certification and maintenance docs stay aligned with the current stable line
