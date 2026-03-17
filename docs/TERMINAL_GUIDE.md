# Terminal Guide

bsj is designed for Terminal.app and iTerm2 on macOS.

## Minimum viable window

- minimum supported size: `80x25`
- intended feel: centered DOS-style `80x25` writing surface even inside a larger window

## Terminal.app suggestions

- use a monospaced font such as Menlo or Monaco
- give the window enough space to avoid the too-small warning
- verify your function keys are not being intercepted by media-key behavior first

## iTerm2 suggestions

- use a clean monospaced font
- confirm function keys are sent through normally
- avoid exotic per-profile key remaps until the base workflow works

## Color behavior

bsj prefers richer color when available, but also supports fallback palettes.

If colors look wrong, first confirm you are not using a terminal profile that aggressively rewrites ANSI colors.

## Cursor behavior

The editor is designed around a block cursor where the terminal supports it.

## If key bindings feel wrong

Try this order:

1. `Esc` to open menus
2. arrow keys to navigate menus
3. `Enter` to trigger actions
4. check the terminal profile for function-key remapping

## Recommended smoke test

1. launch `bsj`
2. verify the centered blue screen appears
3. type a line
4. press `F1`
5. press `F3`
6. press `F7`
7. press `F10`

If those work, your terminal profile is usually configured well enough for daily use.
