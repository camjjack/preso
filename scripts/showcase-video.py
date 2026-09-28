#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow>=11", "numpy>=2", "pygments>=2.18", "fonttools>=4.50"]
# ///
"""Render the README's showcase video: a deck's markdown on the left, the
slide preso makes of it on the right, walked slide by slide and step by step.

    uv run scripts/showcase-video.py            # docs/example-talk.md
    uv run scripts/showcase-video.py --deck talk.md --out talk.mp4

The slides are rendered by this repo's own preso (`--export-pptx
--export-steps`, headless), so the video always matches the current renderer.
Needs ffmpeg on PATH. The editor pane uses the bundled JetBrains Mono and, for
the characters it lacks (CJK, Arabic, emoji), macOS system fonts.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import io
import re
import subprocess
import sys
import tempfile
import zipfile
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np
from fontTools.ttLib import TTFont
from PIL import Image, ImageDraw, ImageFilter, ImageFont
from pygments import lexers, token
from pygments.util import ClassNotFound

REPO = Path(__file__).resolve().parent.parent
FONTS = REPO / "assets" / "fonts"
MONO = FONTS / "JetBrainsMono-Regular.ttf"
UI = FONTS / "Inter-Regular.ttf"
UI_BOLD = FONTS / "Inter-Bold.ttf"
LOGO = REPO / "assets" / "icons" / "preso-128.png"
# Fallbacks for what JetBrains Mono doesn't cover, tried in order.
UNICODE_FALLBACKS = [
    Path("/System/Library/Fonts/Supplemental/Arial Unicode.ttf"),
    Path("/Library/Fonts/Arial Unicode.ttf"),
    Path("/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf"),
]
EMOJI_FONTS = [
    Path("/System/Library/Fonts/Apple Color Emoji.ttc"),
    Path("/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf"),
]

W, H, FPS = 1920, 1080, 30
# JetBrains Mono's ligatures would draw `<!--`, `-->` and `->` as arrows,
# hiding the very syntax the video shows.
NO_LIGATURES = ["-liga", "-calt"]

# Catppuccin Mocha, which the dark theme's code blocks sit well beside.
C = {
    "crust": "#11111b", "mantle": "#181825", "base": "#1e1e2e",
    "surface0": "#313244", "surface1": "#45475a", "overlay0": "#6c7086",
    "overlay2": "#9399b2", "subtext0": "#a6adc8", "text": "#cdd6f4",
    "lavender": "#b4befe", "blue": "#89b4fa", "sapphire": "#74c7ec",
    "teal": "#94e2d5", "green": "#a6e3a1", "yellow": "#f9e2af",
    "peach": "#fab387", "red": "#f38ba8", "mauve": "#cba6f7",
    "pink": "#f5c2e7", "flamingo": "#f2cdcd",
}

# Layout: the editor card on the left, the slide on the right.
MARGIN = 40
CARD = (MARGIN, MARGIN, 900, H - 2 * MARGIN)  # x, y, w, h
TITLE_BAR = 46
SLIDE_W = W - CARD[2] - 3 * MARGIN
SLIDE_H = round(SLIDE_W * 9 / 16)
SLIDE_X = CARD[0] + CARD[2] + MARGIN
SLIDE_Y = (H - SLIDE_H) // 2

# Timing, in seconds.
HOLD = 2.4  # a slide with nothing to step through
FIRST_HOLD = 3.4  # the opening slide, while the viewer takes in the layout
STEP_HOLD = 1.5  # each reveal step
FADE = 0.4  # crossfade + editor scroll between shots
DIM = 0.28  # brightness of lines outside the current slide
UNREVEALED = 0.5  # brightness of this slide's lines a step hasn't reached


# ---------------------------------------------------------------- the deck


@dataclass
class Chunk:
    """One slide's lines in the deck file (0-based, end exclusive). `shown`
    starts earlier for the first slide, which the frontmatter belongs to."""

    start: int
    end: int
    shown: int
    steps: int = 1
    pages: list[Image.Image] = field(default_factory=list)


def fence_of(line: str) -> str | None:
    m = re.match(r"\s*(`{3,}|~{3,})", line)
    return m.group(1) if m else None


def split_deck(lines: list[str]) -> tuple[int, list[Chunk]]:
    """Frontmatter length and the slides, split on `---` outside fences —
    the same rule as preso's parser."""
    body = 0
    if lines and lines[0].strip() == "---":
        for i in range(1, len(lines)):
            if lines[i].strip() == "---":
                body = i + 1
                break
    chunks, start, fence = [], body, None
    for i in range(body, len(lines)):
        f = fence_of(lines[i])
        if fence is None and f:
            fence = f
        elif fence and f and f[0] == fence[0] and len(f) >= len(fence) and not lines[i].strip()[len(f):].strip():
            fence = None
        elif fence is None and lines[i].strip() == "---":
            chunks.append(Chunk(start, i, start))
            start = i + 1
    chunks.append(Chunk(start, len(lines), start))
    chunks = [c for c in chunks if any(l.strip() for l in lines[c.start:c.end])]
    chunks[0].shown = 0
    return body, chunks


def export_pages(preso: Path, deck: Path, steps: bool) -> list[Image.Image]:
    """Render a deck headless, one image per page."""
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "deck.pptx"
        args = [str(preso), str(deck), "--export-pptx", str(out),
                "--export-width", "1920", "--export-quality", "95"]
        if steps:
            args.append("--export-steps")
        subprocess.run(args, check=True, capture_output=True)
        with zipfile.ZipFile(out) as z:
            media = [n for n in z.namelist() if re.fullmatch(r"ppt/media/image\d+\.\w+", n)]
            media.sort(key=lambda n: int(re.search(r"(\d+)\.\w+$", n).group(1)))
            return [Image.open(io.BytesIO(z.read(n))).convert("RGB") for n in media]


def render_slides(preso: Path, deck: Path, lines: list[str], body: int, chunks: list[Chunk]) -> None:
    """Fill in each slide's rendered steps. The whole deck renders once, so
    slide numbers are right; a one-slide deck per slide tells how many of
    those pages each slide owns."""
    frontmatter = "".join(lines[:body])

    def count(chunk: Chunk) -> int:
        with tempfile.NamedTemporaryFile("w", suffix=".md", dir=deck.parent, prefix=".showcase-", delete=False) as f:
            f.write(frontmatter + "".join(lines[chunk.start:chunk.end]))
        try:
            return len(export_pages(preso, Path(f.name), steps=True))
        finally:
            Path(f.name).unlink()

    with concurrent.futures.ThreadPoolExecutor(4) as pool:
        counts = list(pool.map(count, chunks))
        pages = pool.submit(export_pages, preso, deck, True).result()
    if sum(counts) != len(pages):
        sys.exit(f"slide split disagrees with preso: {sum(counts)} steps vs {len(pages)} pages")
    at = 0
    for chunk, n in zip(chunks, counts):
        chunk.steps, chunk.pages = n, pages[at:at + n]
        at += n


# ------------------------------------------------------------ highlighting

Span = tuple[str, str]  # text, colour

INLINE = re.compile(
    r"(?P<code>`[^`]+`)"
    r"|(?P<image>!\[[^\]]*\]\([^)]*\)(?:\{[^}]*\})?)"
    r"|(?P<link>\[[^\]]*\]\([^)]*\))"
    r"|(?P<bold>\*\*[^*]+\*\*)"
    r"|(?P<italic>\*[^*\s][^*]*\*)"
    r"|(?P<math>\$[^$]+\$)"
    r"|(?P<comment><!--.*?-->)"
)
INLINE_COLOUR = {"code": "green", "image": "sapphire", "link": "blue", "bold": "peach",
                 "italic": "flamingo", "math": "teal", "comment": "mauve"}


def inline(text: str, base: str = "text") -> list[Span]:
    spans, at = [], 0
    for m in INLINE.finditer(text):
        if m.start() > at:
            spans.append((text[at:m.start()], C[base]))
        spans.append((m.group(), C[INLINE_COLOUR[m.lastgroup]]))
        at = m.end()
    if at < len(text):
        spans.append((text[at:], C[base]))
    return spans


def directive(line: str) -> list[Span]:
    m = re.match(r"(\s*<!--\s*)([\w-]+(?:\[\d+\])?:?)(.*?)(\s*-->\s*)$", line)
    if not m:
        return [(line, C["mauve"])]
    lead, key, rest, tail = m.groups()
    rest_colour = C["subtext0"] if key.startswith("note") else C["yellow"]
    return [(lead, C["overlay0"]), (key, C["pink"]), (rest, rest_colour), (tail, C["overlay0"])]


TOKEN_COLOUR = [
    (token.Comment, "overlay0"), (token.Keyword, "mauve"), (token.Name.Function, "blue"),
    (token.Name.Class, "yellow"), (token.Name.Builtin, "red"), (token.Name.Tag, "blue"),
    (token.String, "green"), (token.Number, "peach"), (token.Operator, "sapphire"),
    (token.Punctuation, "overlay2"), (token.Name.Attribute, "yellow"),
]


def code_line_spans(lang: str, body: list[str]) -> list[list[Span]]:
    """A fenced block's lines, highlighted by pygments where it knows the
    language."""
    try:
        lexer = lexers.get_lexer_by_name(lang)
    except ClassNotFound:
        return [[(l, C["text"])] for l in body]
    rows: list[list[Span]] = [[]]
    for kind, value in lexer.get_tokens("\n".join(body)):
        colour = next((C[c] for t, c in TOKEN_COLOUR if kind in t), C["text"])
        for i, part in enumerate(value.split("\n")):
            if i:
                rows.append([])
            if part:
                rows[-1].append((part, colour))
    return (rows + [[]] * len(body))[:len(body)]


def highlight(lines: list[str], body: int) -> list[list[Span]]:
    out: list[list[Span]] = [None] * len(lines)  # type: ignore[list-item]
    for i in range(body):
        m = re.match(r"(\w+)(:\s*)(.*)", lines[i])
        out[i] = [(m[1], C["blue"]), (m[2], C["overlay2"]), (m[3], C["green"])] if m else [(lines[i], C["overlay0"])]
    i, math = body, False
    while i < len(lines):
        line = lines[i]
        f = fence_of(line)
        if f:
            info = line.strip()[len(f):]
            lang = info.split("{")[0].strip()
            out[i] = [(line[:line.index(f) + len(f)], C["overlay0"]), (lang, C["yellow"]),
                      (info[len(lang):], C["teal"])]
            j = i + 1
            while j < len(lines) and not (fence_of(lines[j]) or "").startswith(f):
                j += 1
            for k, spans in enumerate(code_line_spans(lang, lines[i + 1:j])):
                out[i + 1 + k] = spans
            if j < len(lines):
                out[j] = [(lines[j], C["overlay0"])]
            i = j + 1
            continue
        s = line.strip()
        if s == "$$":
            math = not math
            out[i] = [(line, C["teal"])]
        elif math:
            out[i] = [(line, C["teal"])]
        elif s == "---":
            out[i] = [(line, C["peach"])]
        elif s == "***":
            out[i] = [(line, C["mauve"])]
        elif s.startswith("<!--"):
            out[i] = directive(line)
        elif m := re.match(r"(#{1,6}\s)(.*)", line):
            out[i] = [(m[1], C["peach"])] + inline(m[2], "lavender")
        elif m := re.match(r"(\s*)([-*+]|\d+\.)(\s.*)", line):
            out[i] = [(m[1], C["text"]), (m[2], C["peach"])] + inline(m[3])
        elif s.startswith("|"):
            if re.fullmatch(r"[|:\-\s]+", s):
                out[i] = [(line, C["overlay0"])]
            else:
                cells = re.split(r"(\|)", line)
                out[i] = [sp for c in cells for sp in ([(c, C["overlay0"])] if c == "|" else inline(c))]
        else:
            out[i] = inline(line)
        i += 1
    return out


# ------------------------------------------------------------------- fonts


class Fonts:
    """JetBrains Mono, with per-character fallback for what it lacks."""

    def __init__(self, size: int):
        self.size = size
        self.mono = ImageFont.truetype(str(MONO), size)
        self.mono_cmap = set(TTFont(MONO).getBestCmap())
        self.fallback = None
        for path in UNICODE_FALLBACKS:
            if path.exists():
                self.fallback = ImageFont.truetype(str(path), size)
                self.fallback_cmap = set(TTFont(path, fontNumber=0).getBestCmap())
                break
        self.emoji_path = next((p for p in EMOJI_FONTS if p.exists()), None)
        self.emoji_cache: dict[str, Image.Image] = {}

    def kind(self, ch: str) -> str:
        cp = ord(ch)
        if cp in self.mono_cmap:
            return "mono"
        if cp >= 0x1F000 or 0x2600 <= cp <= 0x27BF:
            return "emoji"
        return "fallback" if self.fallback and cp in self.fallback_cmap else "mono"

    def emoji(self, ch: str) -> Image.Image | None:
        if not self.emoji_path:
            return None
        if ch not in self.emoji_cache:
            font = ImageFont.truetype(str(self.emoji_path), 160 if "Apple" in self.emoji_path.name else 109)
            glyph = Image.new("RGBA", (220, 220))
            ImageDraw.Draw(glyph).text((10, 10), ch, font=font, embedded_color=True)
            box = glyph.getbbox()
            side = round(self.size * 1.15)
            self.emoji_cache[ch] = glyph.crop(box).resize((side, side), Image.LANCZOS) if box else None
        return self.emoji_cache[ch]

    def runs(self, text: str) -> list[tuple[str, str]]:
        """Split text into runs of one font kind (variation selectors dropped)."""
        runs: list[tuple[str, str]] = []
        for ch in text.replace("️", ""):
            k = self.kind(ch)
            if runs and runs[-1][0] == k and k != "emoji":
                runs[-1] = (k, runs[-1][1] + ch)
            else:
                runs.append((k, ch))
        return runs

    def width(self, kind: str, text: str) -> float:
        if kind == "emoji":
            return round(self.size * 1.15) + 2
        font = self.mono if kind == "mono" else self.fallback
        return font.getlength(text, direction=rtl(text), features=NO_LIGATURES)

    def draw(self, img: Image.Image, xy: tuple[float, float], kind: str, text: str, fill: str) -> None:
        x, y = xy
        if kind == "emoji":
            glyph = self.emoji(text)
            if glyph:
                img.paste(glyph, (round(x) + 1, round(y + self.size * 0.02)), glyph)
            return
        font = self.mono if kind == "mono" else self.fallback
        ImageDraw.Draw(img).text((x, y), text, font=font, fill=fill, direction=rtl(text), features=NO_LIGATURES)


def rtl(text: str) -> str | None:
    return "rtl" if re.search(r"[֐-ࣿ]", text) else None


# ------------------------------------------------------------------ editor


@dataclass
class Row:
    line: int  # the deck line this visual row belongs to
    first: bool  # the line's first row (gets the line number)
    pieces: list[tuple[float, str, str, str]]  # x, font kind, text, colour


def layout(spans_by_line: list[list[Span]], fonts: Fonts, width: float) -> list[Row]:
    """Soft-wrap each line to the pane, breaking after spaces where it can."""
    rows: list[Row] = []
    indent = fonts.mono.getlength("  ")
    for n, spans in enumerate(spans_by_line):
        pieces: list[tuple[str, str, str, float]] = []  # kind, text, colour, width
        for text, colour in spans:
            for kind, run in fonts.runs(text):
                # Words, so a wrap can fall between them.
                for word in (re.findall(r"\S+\s*|\s+", run) if kind != "emoji" else [run]):
                    pieces.append((kind, word, colour, fonts.width(kind, word)))
        row, x = Row(n, True, []), 0.0
        for kind, word, colour, w in pieces:
            if x + w > width and x > indent + 1:
                rows.append(row)
                row, x = Row(n, False, []), indent
            while kind != "emoji" and w > width - x and len(word) > 1:
                # A single word longer than the row: break it by characters.
                cut = len(word)
                while cut > 1 and fonts.width(kind, word[:cut]) > width - x:
                    cut -= 1
                row.pieces.append((x, kind, word[:cut], colour))
                rows.append(row)
                row, x = Row(n, False, []), indent
                word = word[cut:]
                w = fonts.width(kind, word)
            row.pieces.append((x, kind, word, colour))
            x += w
        rows.append(row)
    return rows


@dataclass
class Editor:
    image: np.ndarray  # every row, drawn once, as float RGB
    rows: list[Row]
    line_h: int
    view_rows: int
    background: np.ndarray


def draw_editor(lines: list[str], spans: list[list[Span]], chunks: list[Chunk]) -> Editor:
    """Pick the largest font the tallest slide fits at, then draw the whole
    file as one tall strip for the frames to scroll through."""
    text_top = CARD[1] + TITLE_BAR + 14
    view_h = CARD[1] + CARD[3] - 14 - text_top
    for size in range(22, 13, -1):
        fonts = Fonts(size)
        char = fonts.mono.getlength("0")
        gutter = char * (len(str(len(lines))) + 2)
        text_w = CARD[2] - 28 - gutter
        rows = layout(spans, fonts, text_w)
        line_h = round(size * 1.5)
        view_rows = view_h // line_h
        tallest = max(sum(1 for r in rows if c.shown <= r.line < c.end) for c in chunks)
        if tallest <= view_rows:
            break
    strip = Image.new("RGB", (CARD[2] - 28, len(rows) * line_h), C["base"])
    draw = ImageDraw.Draw(strip)
    for i, row in enumerate(rows):
        y = i * line_h + (line_h - size) // 2 - 2
        if row.first:
            number = str(row.line + 1)
            draw.text((gutter - char * (len(number) + 1), y), number, font=fonts.mono, fill=C["surface1"])
        for x, kind, text, colour in row.pieces:
            fonts.draw(strip, (gutter + x, y), kind, text, colour)
    background = np.array(Image.new("RGB", (1, 1), C["base"]), dtype=np.float32)[0, 0]
    return Editor(np.asarray(strip, dtype=np.float32), rows, line_h, view_rows, background)


def scroll_for(editor: Editor, chunk: Chunk) -> float:
    """Where the editor sits for a slide: its first line near the top, with
    the `---` before it in view."""
    first = next(i for i, r in enumerate(editor.rows) if r.line >= chunk.shown)
    top = max(first - 1, 0)
    return float(min(top, max(len(editor.rows) - editor.view_rows, 0)) * editor.line_h)


def unrevealed(lines: list[str], chunk: Chunk, step: int) -> set[int]:
    """Lines of the slide a reveal step hasn't reached yet: what follows the
    next `<!-- pause -->`, or a code walk's lines outside the current stage."""
    hidden: set[int] = set()
    pauses = [i for i in range(chunk.start, chunk.end) if re.match(r"\s*<!--\s*pause\s*-->", lines[i])]
    if pauses and len(pauses) == chunk.steps - 1 and step < len(pauses):
        hidden |= set(range(pauses[step] + 1, chunk.end))
    i = chunk.start
    while i < chunk.end:
        f = fence_of(lines[i])
        if not f:
            i += 1
            continue
        j = i + 1
        while j < chunk.end and not (fence_of(lines[j]) or "").startswith(f):
            j += 1
        m = re.search(r"\{([^}]*)\}", lines[i])
        stages = m[1].split("|") if m and "|" in m[1] else []
        if len(stages) == chunk.steps and step < len(stages):
            stage = stages[step].replace("zoom", "").strip()
            if stage != "all" and re.fullmatch(r"[\d,\-\s]+", stage):
                keep = set()
                for part in stage.split(","):
                    a, _, b = part.strip().partition("-")
                    keep |= set(range(int(a), int(b or a) + 1))
                hidden |= {i + k for k in range(1, j - i) if k not in keep}
        i = j + 1
    return hidden


def brightness(editor: Editor, lines: list[str], chunk: Chunk, step: int) -> np.ndarray:
    """Per-row brightness for a shot."""
    hidden = unrevealed(lines, chunk, step)
    return np.array([
        (UNREVEALED if r.line in hidden else 1.0) if chunk.shown <= r.line < chunk.end else DIM
        for r in editor.rows
    ], dtype=np.float32)


# ------------------------------------------------------------------ frames


def rounded_mask(w: int, h: int, r: int) -> np.ndarray:
    mask = Image.new("L", (w * 4, h * 4))
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, w * 4 - 1, h * 4 - 1), r * 4, fill=255)
    return np.asarray(mask.resize((w, h), Image.LANCZOS), dtype=np.float32)[..., None] / 255


def backdrop(deck_name: str) -> Image.Image:
    """Everything that doesn't move: the editor card and the slide's frame."""
    img = Image.new("RGB", (W, H), C["crust"])
    shadow = Image.new("L", (W, H))
    sd = ImageDraw.Draw(shadow)
    x, y, w, h = CARD
    sd.rounded_rectangle((x, y + 10, x + w, y + h + 10), 16, fill=150)
    sd.rounded_rectangle((SLIDE_X, SLIDE_Y + 12, SLIDE_X + SLIDE_W, SLIDE_Y + SLIDE_H + 12), 12, fill=170)
    img.paste(Image.new("RGB", (W, H), "#000000"), (0, 0), shadow.filter(ImageFilter.GaussianBlur(18)))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((x, y, x + w, y + h), 16, fill=C["base"], outline=C["surface0"], width=2)
    d.rounded_rectangle((x + 1, y + 1, x + w - 1, y + TITLE_BAR), 15, fill=C["mantle"])
    d.rectangle((x + 1, y + TITLE_BAR - 16, x + w - 1, y + TITLE_BAR), fill=C["mantle"])
    d.line((x + 1, y + TITLE_BAR, x + w - 1, y + TITLE_BAR), fill=C["surface0"], width=2)
    for i, colour in enumerate(("#f38ba8", "#f9e2af", "#a6e3a1")):
        cx, cy = x + 26 + i * 22, y + TITLE_BAR // 2
        d.ellipse((cx - 6, cy - 6, cx + 6, cy + 6), fill=colour)
    ui = ImageFont.truetype(str(UI), 17)
    d.text((x + w / 2, y + TITLE_BAR / 2), deck_name, font=ui, fill=C["subtext0"], anchor="mm")

    # Over the slide: the app's icon and name, so the right side says what
    # it is.
    bold = ImageFont.truetype(str(UI_BOLD), 34)
    logo = Image.open(LOGO).convert("RGBA").resize((56, 56), Image.LANCZOS)
    top = SLIDE_Y - 112
    img.paste(logo, (SLIDE_X, top), logo)
    d.text((SLIDE_X + 70, top + 28), "preso", font=bold, fill=C["text"], anchor="lm")
    return img


@dataclass
class Shot:
    chunk: Chunk
    index: int  # slide number, 1-based
    step: int
    seconds: float


def caption(shot: Shot, total: int) -> Image.Image:
    """The line under the slide: which slide, and which step of it."""
    img = Image.new("RGB", (SLIDE_W, 40), C["crust"])
    d = ImageDraw.Draw(img)
    ui = ImageFont.truetype(str(UI), 20)
    d.text((0, 20), f"Slide {shot.index} of {total}", font=ui, fill=C["overlay2"], anchor="lm")
    if shot.chunk.steps > 1:
        d.text((SLIDE_W, 20), f"step {shot.step + 1} of {shot.chunk.steps}   →", font=ui,
               fill=C["peach"], anchor="rm")
    return img


def ease(t: float) -> float:
    t = min(max(t, 0.0), 1.0)
    return t * t * (3 - 2 * t)


def frames(shots: list[Shot], editor: Editor, lines: list[str], deck_name: str, total: int):
    """Yield every frame as RGB bytes."""
    base = np.asarray(backdrop(deck_name), dtype=np.float32)
    mask = rounded_mask(SLIDE_W, SLIDE_H, 12)
    view_y = CARD[1] + TITLE_BAR + 14
    view_x = CARD[0] + 14
    view_h = editor.view_rows * editor.line_h
    strip_h = editor.image.shape[0]
    row_of_px = np.minimum(np.arange(strip_h) // editor.line_h, len(editor.rows) - 1)

    def state(shot: Shot):
        slide = shot.chunk.pages[shot.step].resize((SLIDE_W, SLIDE_H), Image.LANCZOS)
        return (np.asarray(slide, dtype=np.float32), scroll_for(editor, shot.chunk),
                brightness(editor, lines, shot.chunk, shot.step),
                np.asarray(caption(shot, total), dtype=np.float32))

    prev = None
    for shot in shots:
        cur = state(shot)
        n = round(shot.seconds * FPS)
        still = None
        for f in range(n):
            t = ease(f / (FADE * FPS)) if prev is not None else 1.0
            if t >= 1.0 and still is not None:
                yield still
                continue
            a = prev if prev is not None and t < 1.0 else cur
            slide = cur[0] if t >= 1.0 else a[0] * (1 - t) + cur[0] * t
            scroll = cur[1] if t >= 1.0 else a[1] * (1 - t) + cur[1] * t
            light = cur[2] if t >= 1.0 else a[2] * (1 - t) + cur[2] * t
            cap = cur[3] if t >= 1.0 else a[3] * (1 - t) + cur[3] * t

            frame = base.copy()
            top = int(round(scroll))
            rows = editor.image[top:top + view_h]
            k = light[row_of_px[top:top + rows.shape[0]]][:, None, None]
            frame[view_y:view_y + rows.shape[0], view_x:view_x + rows.shape[1]] = (
                editor.background + (rows - editor.background) * k)
            region = frame[SLIDE_Y:SLIDE_Y + SLIDE_H, SLIDE_X:SLIDE_X + SLIDE_W]
            region[:] = region * (1 - mask) + slide * mask
            frame[SLIDE_Y + SLIDE_H + 22:SLIDE_Y + SLIDE_H + 62, SLIDE_X:SLIDE_X + SLIDE_W] = cap
            out = np.clip(frame, 0, 255).astype(np.uint8).tobytes()
            if t >= 1.0:
                still = out
            yield out
        prev = cur


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--deck", type=Path, default=REPO / "docs" / "example-talk.md")
    ap.add_argument("--out", type=Path, default=REPO / "docs" / "showcase.mp4")
    ap.add_argument("--poster", type=Path, default=REPO / "docs" / "assets" / "showcase.png",
                    help="a still of one slide, for the README to link to the video")
    ap.add_argument("--poster-slide", type=int, default=3, help="which slide the poster shows (1-based)")
    ap.add_argument("--preso", type=Path, help="the preso binary (default: build this repo's release)")
    args = ap.parse_args()

    preso = args.preso
    if preso is None:
        subprocess.run(["cargo", "build", "--release", "-q", "-p", "preso-app"], cwd=REPO, check=True)
        preso = REPO / "target" / "release" / "preso"
    deck = args.deck.resolve()
    lines = deck.read_text().splitlines(keepends=True)
    body, chunks = split_deck(lines)
    print(f"rendering {len(chunks)} slides with {preso}")
    render_slides(preso, deck, lines, body, chunks)

    plain = [l.rstrip("\n") for l in lines]
    editor = draw_editor(plain, highlight(plain, body), chunks)
    shots = []
    for index, chunk in enumerate(chunks, 1):
        # An export can't play a clip, so a video slide would only show its
        # ▶ badge — and the deck's own video slide is this video.
        if any(re.match(r"\s*<!--\s*video:", l) for l in lines[chunk.start:chunk.end]):
            continue
        for step in range(chunk.steps):
            hold = STEP_HOLD if chunk.steps > 1 and step < chunk.steps - 1 else HOLD
            shots.append(Shot(chunk, index, step, FIRST_HOLD if index == 1 and step == 0 else hold))
    seconds = sum(s.seconds for s in shots)
    print(f"{len(shots)} shots, {seconds:.0f}s")

    args.out.parent.mkdir(parents=True, exist_ok=True)
    ffmpeg = subprocess.Popen(
        ["ffmpeg", "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24",
         "-s", f"{W}x{H}", "-r", str(FPS), "-i", "-",
         "-c:v", "libx264", "-preset", "slow", "-crf", "22", "-tune", "stillimage",
         "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(args.out)],
        stdin=subprocess.PIPE)
    for frame in frames(shots, editor, plain, deck.name, len(chunks)):
        ffmpeg.stdin.write(frame)
    ffmpeg.stdin.close()
    if ffmpeg.wait():
        sys.exit("ffmpeg failed")
    print(f"wrote {args.out}")

    poster = next(s for s in shots if s.index == args.poster_slide)
    still = list(frames([Shot(poster.chunk, poster.index, 0, 1 / FPS)], editor, plain, deck.name, len(chunks)))[0]
    args.poster.parent.mkdir(parents=True, exist_ok=True)
    Image.frombytes("RGB", (W, H), still).save(args.poster, optimize=True)
    print(f"wrote {args.poster}")


if __name__ == "__main__":
    main()
