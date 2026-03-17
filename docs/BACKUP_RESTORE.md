# Backup And Restore

Backups are only useful if you know how to restore them before an emergency.

## Create a backup

```bash
bsj backup
```

This creates an encrypted snapshot under:

```text
<vault>/backups/
```

## Inspect backup inventory

```bash
bsj backup list
bsj backup prune
```

Use `backup prune` without `--apply` first to understand retention impact.

## Apply pruning

```bash
bsj backup prune --apply
```

## Restore safely

Restore into a different directory first:

```bash
bsj restore ~/Documents/BlueScreenJournal/backups/<file>.bsjbak.enc --into ~/Documents/BlueScreenJournal-Restore
```

Do not restore directly on top of the only copy of your current vault unless you are deliberately replacing it.

## Recommended backup drill

Run this every so often:

1. create a fresh backup
2. restore it into a temp location
3. confirm the restored vault has the files you expect
4. open the restored vault separately if needed
5. delete the drill restore when finished

## Good retention habits

- keep enough daily backups to cover recent mistakes
- keep enough weekly backups to cover delayed discovery of corruption or deletion
- keep enough monthly backups to cover major recovery events

## If `verify` is broken

Create a backup of the current state before making recovery decisions. Even a damaged vault may still contain recoverable encrypted history.
