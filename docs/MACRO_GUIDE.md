# Macro Guide

Macros are meant to accelerate repeated writing tasks without turning bsj into a shell runner.

## Design intent

Macros are intentionally narrow.

They support:

- inserting template text
- running selected internal commands

They do not exist to execute arbitrary shell commands by default.

## Where macros live

Macros are configured in the app config.

Inspect settings with:

```bash
bsj settings
```

## Example ideas

- insert a daily header
- insert a closing-thought prompt
- jump back to today's date

## Internal commands currently supported

- `insert_date_header`
- `insert_closing_line`
- `jump_today`

## Template example

Conceptually:

```json
{
  "key": "ctrl-j",
  "action": {
    "type": "InsertTemplate",
    "text": "Morning notes:\n- \n"
  }
}
```

## Command example

Conceptually:

```json
{
  "key": "ctrl-t",
  "action": {
    "type": "Command",
    "command": "jump_today"
  }
}
```

## Key-binding advice

- avoid keys that conflict with the app's core workflow
- keep `F1` through `F12` reserved for product commands
- prefer a small number of reliable macros instead of a large unstable set

## Good macro candidates

- journal structure you use repeatedly
- predictable text scaffolds
- navigation shortcuts you invoke every day

## Bad macro candidates

- anything that should be a first-class product feature instead
- anything that hides too much state change behind one key
- anything that would be dangerous if triggered accidentally
