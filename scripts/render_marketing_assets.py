#!/usr/bin/env python3

from __future__ import annotations

from pathlib import Path
from typing import Iterable

from PIL import Image, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "docs" / "assets"
FONT_PATH = Path("/System/Library/Fonts/Menlo.ttc")

CANVAS_W = 1400
CANVAS_H = 980
FRAME_W = 1160
FRAME_H = 820
SCREEN_COLS = 80
SCREEN_ROWS = 25
RELEASE_LABEL = "v2.2.0"

BG_OUTER = "#0b1021"
BG_TERMINAL = "#111827"
BG_TITLEBAR = "#1f2937"
BG_SCREEN = "#0c32b7"
BG_MENU = "#133fcf"
BG_FOOTER = "#1447d4"
BG_OVERLAY = "#07257f"
BG_OVERLAY_TITLE = "#0a2d97"
FG = "#f8fbff"
FG_DIM = "#b4c6ff"
FG_ACCENT = "#fff7c2"
FG_SUCCESS = "#b8ffd1"
FG_ALERT = "#ffe69b"
FG_HILITE = "#0c32b7"
BG_HILITE = "#f8fbff"
BG_MATCH = "#f8fbff"
FG_MATCH = "#08246e"
DOT_RED = "#ef4444"
DOT_YELLOW = "#f59e0b"
DOT_GREEN = "#22c55e"


def font(size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    if FONT_PATH.exists():
        return ImageFont.truetype(str(FONT_PATH), size=size)
    return ImageFont.load_default()


FONT_NORMAL = font(22)
FONT_SMALL = font(19)
FONT_TINY = font(16)


def char_metrics() -> tuple[int, int]:
    box = FONT_NORMAL.getbbox("M")
    return box[2] - box[0], box[3] - box[1] + 6


CHAR_W, LINE_H = char_metrics()
GRID_W = SCREEN_COLS * CHAR_W
GRID_H = SCREEN_ROWS * LINE_H
SCREEN_X = (CANVAS_W - GRID_W) // 2
SCREEN_Y = 130
TERM_X = (CANVAS_W - FRAME_W) // 2
TERM_Y = 80


def row_xy(row: int, col: int = 0) -> tuple[int, int]:
    return SCREEN_X + col * CHAR_W, SCREEN_Y + row * LINE_H


def draw_terminal_base(draw: ImageDraw.ImageDraw, title: str, subtitle: str) -> None:
    draw.rounded_rectangle(
        (TERM_X, TERM_Y, TERM_X + FRAME_W, TERM_Y + FRAME_H),
        radius=28,
        fill=BG_TERMINAL,
    )
    draw.rounded_rectangle(
        (TERM_X + 12, TERM_Y + 12, TERM_X + FRAME_W - 12, TERM_Y + FRAME_H - 12),
        radius=22,
        fill=BG_TITLEBAR,
    )
    draw.rounded_rectangle(
        (TERM_X + 12, TERM_Y + 56, TERM_X + FRAME_W - 12, TERM_Y + FRAME_H - 12),
        radius=18,
        fill=BG_SCREEN,
    )

    dot_y = TERM_Y + 33
    for idx, color in enumerate((DOT_RED, DOT_YELLOW, DOT_GREEN)):
        x = TERM_X + 34 + idx * 24
        draw.ellipse((x, dot_y - 8, x + 16, dot_y + 8), fill=color)

    draw.text((TERM_X + 110, TERM_Y + 20), title, font=FONT_SMALL, fill=FG)
    draw.text((TERM_X + FRAME_W - 360, TERM_Y + 20), subtitle, font=FONT_TINY, fill=FG_DIM)


def fill_screen_rows(draw: ImageDraw.ImageDraw) -> None:
    for row in range(SCREEN_ROWS):
        x0, y0 = row_xy(row)
        fill = BG_SCREEN
        if row == 1:
            fill = BG_MENU
        elif row == SCREEN_ROWS - 1:
            fill = BG_FOOTER
        draw.rectangle((x0 - 10, y0 - 1, x0 + GRID_W + 10, y0 + LINE_H - 2), fill=fill)


def draw_text(draw: ImageDraw.ImageDraw, row: int, text: str, fill: str = FG, col: int = 0) -> None:
    x, y = row_xy(row, col)
    draw.text((x, y), text, font=FONT_NORMAL, fill=fill)


def draw_highlight_span(
    draw: ImageDraw.ImageDraw,
    row: int,
    start_col: int,
    text: str,
    fg: str = FG_HILITE,
    bg: str = BG_HILITE,
) -> None:
    x, y = row_xy(row, start_col)
    w = max(1, len(text)) * CHAR_W
    draw.rectangle((x - 2, y - 1, x + w + 2, y + LINE_H - 3), fill=bg)
    draw.text((x, y), text, font=FONT_NORMAL, fill=fg)


def draw_overlay(
    draw: ImageDraw.ImageDraw,
    title: str,
    lines: Iterable[str],
    width_cols: int,
    height_rows: int,
    start_col: int,
    start_row: int,
) -> None:
    x, y = row_xy(start_row, start_col)
    w = width_cols * CHAR_W
    h = height_rows * LINE_H
    draw.rectangle((x - 14, y - 10, x + w + 14, y + h + 10), fill=BG_OVERLAY, outline=FG, width=2)
    draw.rectangle((x - 14, y - 10, x + w + 14, y + LINE_H + 2), fill=BG_OVERLAY_TITLE)
    draw.text((x, y - 2), title, font=FONT_NORMAL, fill=FG_ACCENT)
    for idx, line in enumerate(lines, start=1):
        draw.text((x, y + idx * LINE_H), line, font=FONT_NORMAL, fill=FG)


def draw_footer(draw: ImageDraw.ImageDraw, labels: list[str]) -> None:
    line = "  ".join(labels)
    draw_text(draw, SCREEN_ROWS - 1, line[:SCREEN_COLS], fill=FG_ACCENT)


def editor_frame() -> Image.Image:
    image = Image.new("RGB", (CANVAS_W, CANVAS_H), BG_OUTER)
    draw = ImageDraw.Draw(image)
    draw_terminal_base(draw, "BlueScreen Journal", "DOS-style encrypted terminal journal")
    fill_screen_rows(draw)

    draw_text(
        draw,
        0,
        f"BLUESCREEN JOURNAL [COMPACT]  TODAY THU 2026-03-19  ENTRY NO. 000477  {RELEASE_LABEL}",
    )
    draw_highlight_span(draw, 1, 0, "FILE")
    draw_text(
        draw,
        1,
        " EDIT  SEARCH  GO  TOOLS  SETUP  HELP  ESC MENUS  F1 HELP  F2 SAVE",
        fill=FG_ACCENT,
        col=4,
    )
    body = [
        "",
        "SEAN'S JOURNAL ENTRY [2026-03-19]",
        "",
        "Today felt quieter than it looked from the outside.",
        "Quick save is simple: type **save** on its own line, press Enter,",
        "and bsj writes a new encrypted revision then opens a clean page.",
        "",
        "Use GO for calendar/index. Use SEARCH for global vault lookup.",
        "Everything here stays encrypted at rest (except explicit exports).",
        "",
        "This keeps the app feeling like a writing appliance, not a dashboard.",
        "",
        "Closing Thought: Build the quiet path into the product itself.",
        "",
    ]
    for idx, line in enumerate(body, start=2):
        draw_text(draw, idx, line, fill=FG)

    cursor_x, cursor_y = row_xy(13, 0)
    draw.rectangle(
        (cursor_x - 2, cursor_y - 1, cursor_x + CHAR_W - 1, cursor_y + LINE_H - 2),
        outline=FG,
        width=2,
    )

    draw_text(
        draw,
        22,
        "STATE WRITE | SAVE READY | LINE 8 COL 4 | WORDS 57 | VER v2.2.0",
        fill=FG_SUCCESS,
    )
    draw_footer(
        draw,
        [
            "F1Hp",
            "F2Sv",
            "F3Dt",
            "F4Fd",
            "F5Sr",
            "F7Ix",
            "F8Sy",
            "F10Qt",
            "F11Rv",
            "F12Lk",
        ],
    )
    return image


def calendar_frame() -> Image.Image:
    image = editor_frame()
    draw = ImageDraw.Draw(image)
    draw_highlight_span(draw, 1, 25, "GO")
    draw_overlay(
        draw,
        "GO -> CALENDAR",
        [
            "March 2026 (Entry Date Jump)",
            "",
            "Su Mo Tu We Th Fr Sa",
            " 1  2  3  4  5  6  7",
            " 8  9 10 11 12 13 14",
            "15 16 17 18 [19] 20 21",
            "22 23 24 25 26 27 28",
            "29 30 31",
            "",
            "PgUp/PgDn month  Enter open  [ ] jump saved day",
        ],
        width_cols=40,
        height_rows=12,
        start_col=20,
        start_row=5,
    )
    draw_text(draw, 18, "Saved days are highlighted. Today is bracketed.", fill=FG_ALERT)
    return image


def search_frame() -> Image.Image:
    image = editor_frame()
    draw = ImageDraw.Draw(image)
    draw_highlight_span(draw, 1, 11, "SEARCH")
    draw_overlay(
        draw,
        "SEARCH -> VAULT",
        [
            "Query: quiet",
            "From: 2026-03-01      To: 2026-03-31",
            "",
            "2026-03-19  Today felt [quiet]er than it looked...",
            "2026-03-18  A [quiet] room makes the draft honest.",
            "2026-03-12  Nothing dramatic. Just a [quiet] win.",
            "",
            "Enter open  Ctrl+Shift+B save preset  Ctrl+1..9 recall",
        ],
        width_cols=58,
        height_rows=10,
        start_col=10,
        start_row=6,
    )
    draw_highlight_span(draw, 3, 11, "quieter", fg=FG_MATCH, bg=BG_MATCH)
    return image


def index_frame() -> Image.Image:
    image = editor_frame()
    draw = ImageDraw.Draw(image)
    draw_highlight_span(draw, 1, 25, "GO")
    draw_overlay(
        draw,
        "GO -> INDEX TIMELINE",
        [
            "Date         Entry No.   Preview",
            "",
            "2026-03-19   000477      Today felt quieter than it looked...",
            "2026-03-18   000476      Setup complete. The screen already feels...",
            "2026-03-15   000473      CONFLICT  Two heads require merge.",
            "",
            "Up/Down move  Enter open  / filter  Shift+S sort toggle",
        ],
        width_cols=66,
        height_rows=9,
        start_col=7,
        start_row=7,
    )
    draw_highlight_span(
        draw,
        10,
        0,
        "2026-03-19   000477      Today felt quieter than it looked...",
        fg=FG_MATCH,
        bg=BG_MATCH,
    )
    return image


def save_pngs(images: dict[str, Image.Image]) -> None:
    ASSETS.mkdir(parents=True, exist_ok=True)
    for name, image in images.items():
        image.save(ASSETS / f"{name}.png")


def save_gif(frames: list[Image.Image]) -> None:
    ASSETS.mkdir(parents=True, exist_ok=True)
    frames[0].save(
        ASSETS / "bsj-hero.gif",
        save_all=True,
        append_images=frames[1:],
        duration=[1200, 1100, 1100, 1200],
        loop=0,
        optimize=True,
        disposal=2,
    )


def main() -> None:
    images = {
        "bsj-editor": editor_frame(),
        "bsj-calendar": calendar_frame(),
        "bsj-search": search_frame(),
        "bsj-index": index_frame(),
    }
    save_pngs(images)
    save_gif(
        [
            images["bsj-editor"],
            images["bsj-calendar"],
            images["bsj-search"],
            images["bsj-index"],
        ]
    )


if __name__ == "__main__":
    main()
