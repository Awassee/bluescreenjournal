# Quickstart

This is the shortest path from install to a working encrypted journal.

## 1. Install

Preferred public install:

```bash
curl -fsSL https://raw.githubusercontent.com/Awassee/bluescreenjournal/main/install.sh | bash
```

## 2. Launch

```bash
bsj
```

If no vault exists yet, the setup wizard asks for:

1. vault path
2. passphrase
3. passphrase confirmation
4. optional epoch date for `ENTRY NO.`

Default vault path:

```text
~/Documents/BlueScreenJournal
```

## 3. Write and save

- type immediately into today's entry
- press `F2` to save an encrypted revision
- press `F10` to quit
- press `F12` to lock

## 4. Reopen and verify

```bash
bsj
bsj verify
bsj settings
```

## 5. Learn the core movement keys

- `F3` open calendar
- `F4` find in entry
- `F5` search vault
- `F7` open index
- `F8` sync
- `F11` reveal codes

## 6. Do one safety check

Make sure you can create a backup:

```bash
bsj backup
bsj backup list
```

## 7. Optional: set up sync

Folder sync example:

```bash
bsj sync --backend folder --remote ~/Library/Mobile\ Documents/com~apple~CloudDocs/BlueScreenJournal
```

## If something feels off

Run:

```bash
bsj doctor
bsj doctor --unlock
```

Then read:

- `docs/FAQ.md`
- `SUPPORT.md`
