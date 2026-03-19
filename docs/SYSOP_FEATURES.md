# SYSOP Feature Pack

BlueScreen Journal includes a dedicated operator toolkit:

1. `bsj sysop dashboard`  
Unified state snapshot for vault health, integrity, conflicts, backups, cache, and hygiene counters.

2. `bsj sysop runbook`  
Action-prioritized checklist generated from current system state.

3. `bsj sysop env`  
Environment readiness for passphrase and sync backends.

4. `bsj sysop paths`  
Resolved config/log/vault/sync paths with existence checks.

5. `bsj sysop permissions`  
Permission and symlink risk audit under vault root.

6. `bsj sysop vault-layout`  
Layout validation for required directories and encrypted file naming.

7. `bsj sysop orphans`  
Unknown-file detector for unsupported paths in vault structure.

8. `bsj sysop revisions`  
Top revision-volume dates to spot heavy edit/rewrite days.

9. `bsj sysop drafts --older-than-days N`  
Stale draft detector (encrypted drafts only).

10. `bsj sysop conflicts`  
Conflict date list for merge triage.

11. `bsj sysop cache`  
Encrypted search-cache health and validity inspection.

12. `bsj sysop backups`  
Backup inventory with retention prune preview.

13. `bsj sysop integrity`  
Hashchain verification with issue reporting.

14. `bsj sysop activity --days N`  
Recent revision-activity series for ops visibility.

15. `bsj sysop sync-preview`  
Non-destructive local/remote sync delta preview (folder, S3, WebDAV).

All sysop commands are read-only except normal config side effects in backend path resolution (same behavior as `bsj sync` for folder target convenience).
