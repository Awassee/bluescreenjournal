# v1.3.3 Patch List

This patch list comes directly from the live public install validation run against the published `v1.3.2` GitHub release on March 20, 2026.

## Findings from live install validation

1. Public bootstrap install was selecting the wrong archive name.
   - Symptom: the raw GitHub installer tried to download `bsj-universal-apple-darwin.tar.gz` and failed with `404`.
   - Impact: fresh-machine public install was broken even though the release assets themselves were valid.
   - Status: fixed on `main` by resolving the correct published asset for the current Mac.

2. Post-install PATH messaging was contradictory.
   - Symptom: installer warned that `bsj` was not on `PATH` and then immediately repaired `PATH` for the installer session.
   - Impact: users got a confusing false-negative signal at the end of install.
   - Status: fixed on `main` by removing the premature warning and reporting the session PATH repair directly.

3. GitHub Raw `main` can lag briefly after a push.
   - Symptom: `raw.githubusercontent.com/.../main/install.sh` still served the previous installer revision immediately after `main` was updated.
   - Impact: a public validation run can fail even though the pushed commit and release assets are already correct.
   - Status: observed during validation; process follow-up needed.

## Remaining v1.3.3 work

1. Add an automated public-release smoke script to the repo.
   - Acceptance:
   - runs `curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash -s -- --prebuilt --version <tag>`
   - installs into a clean temp `HOME`
   - verifies `bsj --version`
   - verifies shell profile PATH updates

2. Wire that public-release smoke into the release checklist and CI/docs.
   - Acceptance:
   - release docs mention the exact command
   - release process includes a step that must pass before calling a release healthy

3. Clarify published-asset policy for Intel vs Apple Silicon.
   - Acceptance:
   - docs explain which stable asset names are expected
   - installer doctor shows which asset it would try to download
   - unsupported/missing targets fail with a direct, human-readable error

4. Add a visible installer line showing the chosen release asset before download.
   - Acceptance:
   - users can see whether the installer picked `aarch64`, `x86_64`, or `universal`
   - output remains concise and menu-first

5. Fresh-machine validation on a second macOS environment.
   - Acceptance:
   - one more real clean-machine install test is recorded after the fix
   - any discrepancies go into the next patch list instead of being lost in chat

6. Add a release runbook note for CDN propagation checks.
   - Acceptance:
   - release docs tell operators to verify both:
     - the pushed commit URL
     - the `main` raw URL after propagation
   - publish is not considered fully healthy until both resolve to the expected installer revision
