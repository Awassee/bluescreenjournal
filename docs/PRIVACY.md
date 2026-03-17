# Privacy And Data Handling

This guide explains the product's privacy posture in plain language.

## Core model

bsj is local-first.

That means the vault on disk is the source of truth. The product is not built around a hosted account system.

## What is intended to be encrypted

- saved journal revisions
- autosave drafts
- backup archives
- sync revision blobs

## What may exist in plaintext

Operational metadata may exist in plaintext where needed for the app to function.

Examples:

- `vault.json` metadata and KDF parameters
- device nickname files
- local config values you choose to persist
- plaintext exports you explicitly create with `bsj export`

## What the product tries to avoid

- silent plaintext journal persistence in the vault
- plaintext search index files on disk
- plaintext journal content in logs

## User-controlled plaintext surfaces

If you choose these actions, plaintext can exist because you explicitly asked for it:

- `bsj export`
- copying text out of the terminal
- screenshots or screen recordings
- redirecting command output to a plaintext file

## Sync privacy model

Sync backends are intended to receive encrypted revision blobs plus required metadata, not plaintext journal bodies.

## Operator precautions

- be careful with `BSJ_PASSPHRASE` in shell history or shell startup files
- be deliberate about where you write exported text
- treat restored vault copies as sensitive data too
- remember that your terminal scrollback is outside bsj's control
