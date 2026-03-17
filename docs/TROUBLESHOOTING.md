# Troubleshooting

This guide is for when bsj is installed but something is not behaving the way you expect.

## Start with diagnostics

Run these first:

```bash
bsj settings
bsj doctor
bsj doctor --unlock
```

These commands usually answer three questions quickly:

- is bsj installed where you think it is
- is the vault path what you expect
- is the encrypted vault healthy enough to open and verify

## Common problems

### `bsj` is not found

The installer prints the exact `PATH` line to add if the install directory is not already on `PATH`.

Typical fix:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

Then add it to `~/.zshrc`.

### The terminal says the window is too small

Resize to at least `80x25`.

bsj intentionally preserves a DOS-style writing surface and warns instead of rendering into a layout that is too cramped to use safely.

### Function keys do not behave correctly

Check your terminal settings first.

- make sure Terminal.app or iTerm2 is sending real function-key events
- if your keyboard has a hardware Fn layer, verify whether macOS is treating the keys as media keys first
- try the menu system via `Esc` and arrow keys if direct function keys are intercepted

### Unlock fails even though the vault exists

Common causes:

- wrong passphrase
- different vault path than expected
- corrupted or replaced `vault.json`
- trying to open a different machine's copied files without the correct passphrase

Run:

```bash
bsj settings
bsj doctor
```

Confirm the vault path first. Then confirm the vault exists there.

### Search returns nothing when entries exist

Global search indexes saved revisions after unlock. If you have unsaved editor changes, they will not appear until they are saved.

Check:

- did you press `F2`
- are you searching within the right date range
- are you searching for text in the body versus only in your unsaved current buffer

### Sync shows conflicts

Conflicts are preserved on purpose.

Use the index to identify the affected date, then open it in the TUI and resolve the merge there. Do not delete revision files manually unless you are deliberately doing vault surgery.

Read `docs/SYNC_GUIDE.md` before changing files by hand.

### `verify` reports BROKEN

Treat this as a real integrity event until proven otherwise.

Possible causes:

- missing revision file
- copied vault with incomplete sync
- manual file tampering
- interrupted external file operations

Immediate steps:

1. stop syncing that vault anywhere else
2. create a copy of the vault directory as-is
3. run `bsj doctor --unlock`
4. review the exact affected date(s)
5. restore from backup if necessary

### Backup restore looks wrong

Always restore into a different directory first.

Good pattern:

```bash
bsj restore ~/Documents/BlueScreenJournal/backups/<file>.bsjbak.enc --into ~/Documents/BlueScreenJournal-Restore
```

Then inspect the restored vault before swapping it into place.

## Safe escalation path

If you still cannot resolve the problem:

1. capture output from `bsj settings`
2. capture output from `bsj doctor`
3. capture output from `bsj doctor --unlock` if safe
4. open `SUPPORT.md`
5. file a GitHub issue without secrets or journal content

## What not to do

- do not paste passphrases into public issues
- do not paste journal content into bug reports
- do not delete encrypted revision blobs just to make a warning disappear
- do not assume a conflict or integrity warning is cosmetic
