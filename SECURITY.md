# Security Policy

## Supported line

The current public release line is supported for security issues.

## Reporting

If you believe you found a vulnerability involving encryption, data leakage, packaged artifacts, installers, or sync behavior, do not file a public issue first.

Report privately to the repository owner through GitHub security reporting if available, or through a private contact path you control.

## Please include

- affected version
- install path used
- exact steps to reproduce
- whether journal plaintext or secrets were exposed
- whether the issue affects vault files, backups, logs, exports, or sync targets

## Scope examples

In scope:

- plaintext journal leakage to disk
- secrets written to logs or config unexpectedly
- release artifact privacy leaks
- integrity verification bypasses
- sync behavior that silently drops encrypted revisions

Out of scope:

- local machine compromise outside bsj
- weak user-chosen passphrases
- intentional plaintext exports created by explicit user action
