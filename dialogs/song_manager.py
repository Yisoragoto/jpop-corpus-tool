"""Song management dialogs."""

from __future__ import annotations

import hashlib
import html as html_lib
import copy
import subprocess
from collections import OrderedDict

from dialogs._bridge import install_gui_symbols

install_gui_symbols(globals())

from PyQt6.QtCore import QUrl, QSize, QRectF, QPointF, pyqtProperty
from PyQt6.QtGui import (
    QBrush, QColor, QFont, QFontDatabase, QFontMetrics, QPainter,
    QTextLayout, QTextLine, QTextOption,
)
from PyQt6.QtMultimedia import QAudioOutput, QMediaPlayer
from PyQt6.QtWidgets import (
    QApplication, QFontComboBox, QListWidget, QListWidgetItem, QSizePolicy, QSlider, QSpinBox,
    QSplitter, QStackedWidget, QTextBrowser, QTreeWidget, QTreeWidgetItem
)

# ------------------------------------------------------------------ SongManagerDialog

_KANA_OFFSET = ord("ぁ") - ord("ァ")
_KANJI_RE = re.compile(r"[\u3400-\u4dbf\u4e00-\u9fff]")
_SUDACHI_TOKENIZER = None
_SUDACHI_MODE = None
_GINZA_NLP = None
_FURIGANA_CACHE: dict[tuple[str, str], list[tuple[str, str]]] = {}
_INSIGHT_INDEXES_READY = False
_WORD_DICT_CACHE: OrderedDict[tuple, dict] = OrderedDict()
_WORD_CORPUS_CACHE: OrderedDict[tuple, dict] = OrderedDict()
_WORD_FULL_CACHE: OrderedDict[tuple, dict] = OrderedDict()
_WORD_TOKEN_CACHE: OrderedDict[str, list[dict]] = OrderedDict()
_WORD_CACHE_LIMIT = 256


def _active_theme() -> str:
    app = QApplication.instance()
    if app is not None and app.property("jpopTheme") in {"dark", "light"}:
        return app.property("jpopTheme")
    return "light" if load_settings().get("theme") == "light" else "dark"


def _lyric_colors(theme: str | None = None) -> dict[str, str]:
    if (theme or _active_theme()) == "light":
        return {
            "bg": "#ffffff", "active_bg": "#dbeafe", "active": "#0f172a",
            "text": "#263244", "ruby": "#64748b", "time": "#718096",
            "active_ruby": "#334155", "muted": "#7a8797",
        }
    return {
        "bg": "#1f2329", "active_bg": "#24394d", "active": "#ffffff",
        "text": "#d7dde7", "ruby": "#96a2b2", "time": "#8fa0b6",
        "active_ruby": "#dfe9f7", "muted": "#8d98a8",
    }


def _clamp_int(value, default: int, low: int, high: int) -> int:
    try:
        return max(low, min(high, int(value)))
    except Exception:
        return default


def _song_display_settings() -> dict:
    cfg = load_settings()
    furigana_mode = cfg.get("furigana_mode", "kanji")
    if furigana_mode not in {"kanji", "word"}:
        furigana_mode = "kanji"
    return {
        "family": (cfg.get("song_lyric_font_family") or cfg.get("font_family") or "Klee One"),
        "fallback_family": (cfg.get("song_lyric_fallback_font_family") or "LXGW WenKai"),
        "base_size": _clamp_int(cfg.get("song_lyric_font_size"), 18, 12, 30),
        "ruby_size": _clamp_int(cfg.get("song_furigana_font_size"), 9, 6, 18),
        "line_spacing": _clamp_int(cfg.get("song_lyric_line_spacing"), 8, 0, 36),
        "letter_spacing": _clamp_int(cfg.get("song_lyric_letter_spacing"), 1, 0, 12),
        "furigana_mode": furigana_mode,
    }


def _lyric_font_families(display: dict) -> list[str]:
    families = []
    for name in (
        display.get("family"),
        display.get("fallback_family"),
        "LXGW WenKai",
        "Klee One",
        "Microsoft YaHei UI",
        "Yu Gothic UI",
    ):
        name = str(name or "").strip()
        if name and name not in families:
            families.append(name)
    return families


def _lyric_font(display: dict, point_size: int | None = None,
                weight: QFont.Weight | None = None) -> QFont:
    families = _lyric_font_families(display)
    font = QFont(families[0] if families else "")
    if hasattr(font, "setFamilies"):
        font.setFamilies(families)
    if point_size is not None:
        font.setPointSize(int(point_size))
    if weight is not None:
        font.setWeight(weight)
    return font


def _token_spans(text: str, tokens: list[dict] | None = None) -> list[tuple[int, int, dict]]:
    """Map tokenizer results back to source character ranges without losing spaces."""
    spans = []
    cursor = 0
    for token in tokens or _word_tokens(text):
        surface = str(token.get("surface", ""))
        if not surface:
            continue
        start = text.find(surface, cursor)
        if start < 0:
            start = cursor
        end = min(len(text), start + len(surface))
        if end > start:
            spans.append((start, end, token))
        cursor = end
    return spans


def _furigana_word_boxes(text: str, cells: list[tuple[str, str]],
                         base_metrics: QFontMetrics, ruby_metrics: QFontMetrics,
                         start_x: float, spacing: int,
                         right: float | None = None) -> list[tuple[float, float, dict]]:
    """Return word hit boxes from the exact furigana cell layout used for painting."""
    char_ranges: dict[int, tuple[float, float]] = {}
    source_cursor = 0
    x = float(start_x)
    for surface, reading in cells:
        base_w = base_metrics.horizontalAdvance(surface)
        ruby_w = ruby_metrics.horizontalAdvance(reading)
        cell_w = max(base_w, ruby_w)
        if right is not None and x + cell_w > right:
            break
        base_left = x + (cell_w - base_w) / 2
        source_start = text.find(surface, source_cursor)
        if source_start < 0:
            source_start = source_cursor
        for offset in range(len(surface)):
            left = base_left + base_metrics.horizontalAdvance(surface[:offset])
            glyph_right = base_left + base_metrics.horizontalAdvance(surface[:offset + 1])
            char_ranges[source_start + offset] = (left, max(left + 1, glyph_right))
        source_cursor = source_start + len(surface)
        x += cell_w + spacing

    boxes = []
    for start, end, token in _token_spans(text):
        ranges = [char_ranges[index] for index in range(start, end) if index in char_ranges]
        if not ranges:
            continue
        boxes.append((min(item[0] for item in ranges), max(item[1] for item in ranges), token))
    return boxes


def _katakana_to_hiragana(text: str) -> str:
    return "".join(
        chr(ord(ch) + _KANA_OFFSET) if "ァ" <= ch <= "ヶ" else ch
        for ch in text
    )


def _is_kana_char(ch: str) -> bool:
    return "\u3040" <= ch <= "\u309f" or "\u30a0" <= ch <= "\u30ff" or ch == "ー"


def _surface_reading_key(text: str) -> str:
    return "".join(_katakana_to_hiragana(ch) for ch in text if _is_kana_char(ch))


def _is_kanji_char(ch: str) -> bool:
    return bool(_KANJI_RE.match(ch)) or ch == "々"


def _split_kanji_runs(surface: str) -> list[tuple[str, bool]]:
    if not surface:
        return []
    parts: list[tuple[str, bool]] = []
    buf = surface[0]
    current_is_kanji = _is_kanji_char(surface[0])
    for ch in surface[1:]:
        is_kanji = _is_kanji_char(ch)
        if is_kanji == current_is_kanji:
            buf += ch
        else:
            parts.append((buf, current_is_kanji))
            buf = ch
            current_is_kanji = is_kanji
    parts.append((buf, current_is_kanji))
    return parts


def _kanji_only_furigana(surface: str, reading_text: str) -> list[tuple[str, str]]:
    if not _KANJI_RE.search(surface) or not reading_text:
        return [(surface, "")]
    reading = _katakana_to_hiragana(reading_text)
    parts = _split_kanji_runs(surface)
    result: list[tuple[str, str]] = []
    cursor = 0
    for index, (part, is_kanji) in enumerate(parts):
        if not is_kanji:
            key = _surface_reading_key(part)
            if key and reading.startswith(key, cursor):
                cursor += len(key)
            elif key:
                found = reading.find(key, cursor)
                if found >= 0:
                    cursor = found + len(key)
            result.append((part, ""))
            continue

        next_key = ""
        for next_part, next_is_kanji in parts[index + 1:]:
            if not next_is_kanji:
                next_key = _surface_reading_key(next_part)
                if next_key:
                    break
        if next_key:
            next_pos = reading.find(next_key, cursor)
            part_reading = reading[cursor:next_pos] if next_pos >= cursor else ""
            if next_pos >= cursor:
                cursor = next_pos
        else:
            part_reading = reading[cursor:]
            cursor = len(reading)
        result.append((part, part_reading if part_reading and part_reading != part else ""))
    return result or [(surface, reading)]


def _furigana_tokens(text: str, mode: str = "kanji") -> list[tuple[str, str]]:
    """Return (surface, reading) pairs. Reading is empty when furigana is unnecessary."""
    global _SUDACHI_TOKENIZER, _SUDACHI_MODE, _GINZA_NLP
    mode = "word" if mode == "word" else "kanji"
    cache_key = (mode, text)
    if cache_key in _FURIGANA_CACHE:
        return _FURIGANA_CACHE[cache_key]
    try:
        if _SUDACHI_TOKENIZER is None:
            from sudachipy import dictionary, tokenizer
            _SUDACHI_TOKENIZER = dictionary.Dictionary().create()
            _SUDACHI_MODE = tokenizer.Tokenizer.SplitMode.C
        result = []
        for token in _SUDACHI_TOKENIZER.tokenize(text, _SUDACHI_MODE):
            surface = token.surface()
            reading = token.reading_form()
            reading_text = _katakana_to_hiragana(reading) if reading and reading != "*" else ""
            if not _KANJI_RE.search(surface) or not reading_text or reading_text == surface:
                reading_text = ""
            if reading_text and mode == "kanji":
                result.extend(_kanji_only_furigana(surface, reading_text))
            else:
                result.append((surface, reading_text))
        if result:
            _FURIGANA_CACHE[cache_key] = result
            return result
    except Exception:
        pass
    try:
        if _GINZA_NLP is None:
            import spacy
            _GINZA_NLP = spacy.load("ja_ginza")
        tokens = []
        for token in _GINZA_NLP(text):
            surface = token.text
            reading = token.morph.get("Reading")
            reading_text = _katakana_to_hiragana(reading[0]) if reading else ""
            if not _KANJI_RE.search(surface) or not reading_text or reading_text == surface:
                reading_text = ""
            if reading_text and mode == "kanji":
                tokens.extend(_kanji_only_furigana(surface, reading_text))
            else:
                tokens.append((surface, reading_text))
        result = tokens or [(text, "")]
    except Exception:
        result = [(text, "")]
    _FURIGANA_CACHE[cache_key] = result
    return result


def _word_tokens(text: str) -> list[dict]:
    global _SUDACHI_TOKENIZER, _SUDACHI_MODE, _GINZA_NLP
    text = text or ""
    if not text:
        return []
    cached = _WORD_TOKEN_CACHE.get(text)
    if cached is not None:
        _WORD_TOKEN_CACHE.move_to_end(text)
        return copy.deepcopy(cached)

    def store(result: list[dict]) -> list[dict]:
        _WORD_TOKEN_CACHE[text] = copy.deepcopy(result)
        _WORD_TOKEN_CACHE.move_to_end(text)
        while len(_WORD_TOKEN_CACHE) > _WORD_CACHE_LIMIT * 4:
            _WORD_TOKEN_CACHE.popitem(last=False)
        return copy.deepcopy(result)

    try:
        if _SUDACHI_TOKENIZER is None:
            from sudachipy import dictionary, tokenizer
            _SUDACHI_TOKENIZER = dictionary.Dictionary().create()
            _SUDACHI_MODE = tokenizer.Tokenizer.SplitMode.C
        result = []
        for token in _SUDACHI_TOKENIZER.tokenize(text, _SUDACHI_MODE):
            surface = token.surface()
            if not surface or surface.isspace():
                continue
            pos = token.part_of_speech()[0] if token.part_of_speech() else ""
            if pos in {"補助記号", "空白"}:
                continue
            lemma = token.dictionary_form() or surface
            reading = token.reading_form()
            result.append({
                "surface": surface,
                "lemma": lemma,
                "reading": _katakana_to_hiragana(reading) if reading and reading != "*" else "",
                "pos": pos,
            })
        if result:
            return store(result)
    except Exception:
        pass
    try:
        if _GINZA_NLP is None:
            import spacy
            _GINZA_NLP = spacy.load("ja_ginza")
        result = []
        for token in _GINZA_NLP(text):
            if token.is_space or token.is_punct:
                continue
            reading = token.morph.get("Reading")
            result.append({
                "surface": token.text,
                "lemma": token.lemma_ or token.text,
                "reading": _katakana_to_hiragana(reading[0]) if reading else "",
                "pos": token.pos_,
            })
        if result:
            return store(result)
    except Exception:
        pass
    return store([{"surface": text, "lemma": text, "reading": "", "pos": ""}])


def _plain_def_text(item) -> str:
    if isinstance(item, str):
        return item
    if isinstance(item, dict):
        content = item.get("content")
        if isinstance(content, (list, dict)):
            return _plain_def_text(content)
        if content:
            return str(content)
        if item.get("text"):
            return str(item.get("text"))
    if isinstance(item, list):
        return " ".join(_plain_def_text(x) for x in item if x)
    return str(item or "")


class ClickableSlider(QSlider):
    clickedValue = pyqtSignal(int)

    def mousePressEvent(self, event):
        if self.orientation() == Qt.Orientation.Horizontal and self.width() > 0:
            ratio = max(0.0, min(event.position().x() / self.width(), 1.0))
            value = self.minimum() + round((self.maximum() - self.minimum()) * ratio)
            self.setValue(value)
            self.clickedValue.emit(value)
        super().mousePressEvent(event)


class FuriganaTextWidget(QWidget):
    def __init__(self, text: str, *, active: bool = False, large: bool = False,
                 display: dict | None = None, parent=None):
        super().__init__(parent)
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, False)
        self.setAutoFillBackground(False)
        self._active = active
        self._large = large
        self._text = text
        self._display = display or _song_display_settings()
        self._tokens = _furigana_tokens(text, self._display.get("furigana_mode", "kanji"))
        self._spacing = max(3, int(self._display["line_spacing"] * 0.45)) + int(self._display.get("letter_spacing", 1))
        if not self._large:
            self._spacing = max(3, self._spacing - 2)
        self.setMinimumHeight(self.sizeHint().height())

    def _fonts(self):
        base_size = int(self._display.get("base_size", 18))
        ruby_size = int(self._display.get("ruby_size", 9))
        ruby_font = _lyric_font(
            self._display,
            ruby_size if self._large else max(6, ruby_size - 2),
            QFont.Weight.Medium,
        )
        base_font = _lyric_font(
            self._display,
            base_size if self._large else max(11, base_size - 4),
            QFont.Weight.Bold if self._active else QFont.Weight.Medium,
        )
        return ruby_font, base_font

    def sizeHint(self):
        ruby_font, base_font = self._fonts()
        ruby_metrics = QFontMetrics(ruby_font)
        base_metrics = QFontMetrics(base_font)
        width = 0
        for surface, reading in self._tokens:
            width += max(base_metrics.horizontalAdvance(surface), ruby_metrics.horizontalAdvance(reading))
            width += self._spacing
        height = ruby_metrics.height() + base_metrics.height() + (6 if self._large else 4)
        return QSize(max(width, 80), height)

    def setActive(self, active: bool):
        self._active = active
        self.updateGeometry()
        self.update()

    def tokenAtX(self, x_pos: float) -> dict:
        ruby_font, base_font = self._fonts()
        boxes = _furigana_word_boxes(
            self._text,
            self._tokens,
            QFontMetrics(base_font),
            QFontMetrics(ruby_font),
            0,
            self._spacing,
        )
        for left, right, token in boxes:
            if left <= x_pos <= right:
                return dict(token)
        if boxes:
            nearest = min(boxes, key=lambda item: min(abs(x_pos - item[0]), abs(x_pos - item[1])))
            if min(abs(x_pos - nearest[0]), abs(x_pos - nearest[1])) <= 3:
                return dict(nearest[2])
        return {}

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.TextAntialiasing, True)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        ruby_font, base_font = self._fonts()
        ruby_metrics = QFontMetrics(ruby_font)
        base_metrics = QFontMetrics(base_font)
        colors = _lyric_colors()
        base_color = QColor(colors["active"] if self._active else colors["text"])
        ruby_color = QColor(colors["active_ruby"] if self._active else colors["ruby"])
        x = 0
        ruby_y = ruby_metrics.ascent()
        base_y = ruby_metrics.height() + base_metrics.ascent() + (4 if self._large else 3)
        for surface, reading in self._tokens:
            base_w = base_metrics.horizontalAdvance(surface)
            ruby_w = ruby_metrics.horizontalAdvance(reading)
            cell_w = max(base_w, ruby_w)
            if reading:
                painter.setFont(ruby_font)
                painter.setPen(ruby_color)
                painter.drawText(int(x + (cell_w - ruby_w) / 2), ruby_y, reading)
            painter.setFont(base_font)
            painter.setPen(base_color)
            painter.drawText(int(x + (cell_w - base_w) / 2), base_y, surface)
            x += cell_w + self._spacing


class LyricLineWidget(QWidget):
    def __init__(self, time_text: str, lyric: str, *, furigana: bool = False,
                 active: bool = False, expanded: bool = False,
                 display: dict | None = None, parent=None):
        super().__init__(parent)
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, True)
        self._time_text = time_text
        self._lyric = lyric
        self._furigana = furigana
        self._expanded = expanded
        self._active = active
        self._display = display or _song_display_settings()
        self._build()

    def _clear(self):
        lay = self.layout()
        if not lay:
            return
        while lay.count():
            item = lay.takeAt(0)
            if item.widget():
                item.widget().deleteLater()
            elif item.layout():
                while item.layout().count():
                    child = item.layout().takeAt(0)
                    if child.widget():
                        child.widget().deleteLater()

    def _build(self):
        if self.layout():
            self._clear()
            layout = self.layout()
        else:
            layout = QHBoxLayout(self)
        layout.setContentsMargins(10 if self._expanded else 8, 8 if self._expanded else 4,
                                  10 if self._expanded else 8, 8 if self._expanded else 4)
        layout.setSpacing(16 if self._expanded else 12)
        self.setObjectName("ActiveLyric" if self._active else "LyricLine")
        time = QLabel(self._time_text)
        time.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
        time.setObjectName("LyricTime")
        time.setFixedWidth(92 if self._expanded else 78)
        time_font = _lyric_font(
            self._display,
            max(9, int(self._display.get("ruby_size", 9)) + (1 if self._expanded else 0)),
        )
        time.setFont(time_font)
        if self._furigana:
            text_widget = FuriganaTextWidget(
                self._lyric, active=self._active, large=self._expanded,
                display=self._display, parent=self
            )
            text_widget.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
            layout.addWidget(time)
            layout.addWidget(text_widget, 1)
        else:
            text = QLabel(self._lyric)
            text.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
            text.setObjectName("LyricText")
            text.setWordWrap(True)
            base_size = int(self._display.get("base_size", 18))
            font = _lyric_font(
                self._display,
                base_size if self._expanded else max(11, base_size - 4),
                QFont.Weight.Bold if self._active else QFont.Weight.Normal,
            )
            text.setFont(font)
            layout.addWidget(time)
            layout.addWidget(text, 1)
        self.setActive(self._active)

    def setActive(self, active: bool):
        self._active = active
        self.setProperty("active", active)
        self.style().unpolish(self)
        self.style().polish(self)
        for ruby in self.findChildren(FuriganaTextWidget):
            ruby.setActive(active)
        colors = _lyric_colors()
        for child in self.findChildren(QLabel):
            if child.objectName() == "LyricTime":
                child.setStyleSheet(f"color:{colors['active_ruby'] if active else colors['time']};")
            elif child.objectName() == "LyricText":
                child.setStyleSheet(f"color:{colors['active'] if active else colors['text']};")

    def tokenAtPosition(self, pos) -> dict:
        ruby = self.findChild(FuriganaTextWidget)
        if ruby is not None and ruby.geometry().adjusted(-4, -4, 4, 4).contains(pos):
            return ruby.tokenAtX(pos.x() - ruby.geometry().left())

        label = next(
            (child for child in self.findChildren(QLabel) if child.objectName() == "LyricText"),
            None,
        )
        if label is None or not label.geometry().adjusted(-4, -4, 4, 4).contains(pos):
            return {}
        local_pos = pos - label.geometry().topLeft()
        char_index = self._label_character_index(label, local_pos)
        if char_index < 0:
            return {}
        for start, end, token in _token_spans(self._lyric):
            if start <= char_index < end:
                return dict(token)
        return {}

    def _label_character_index(self, label: QLabel, pos) -> int:
        rect = label.contentsRect()
        if not rect.adjusted(-3, -3, 3, 3).contains(pos):
            return -1
        layout = QTextLayout(self._lyric, label.font())
        option = QTextOption()
        option.setWrapMode(QTextOption.WrapMode.WrapAtWordBoundaryOrAnywhere)
        layout.setTextOption(option)
        layout.beginLayout()
        lines = []
        y = 0.0
        line_width = max(1, rect.width())
        while True:
            line = layout.createLine()
            if not line.isValid():
                break
            line.setLineWidth(line_width)
            line.setPosition(QPointF(0, y))
            lines.append(line)
            y += line.height()
        layout.endLayout()

        vertical_offset = max(0.0, (rect.height() - y) / 2)
        x = pos.x() - rect.left()
        target_y = pos.y() - rect.top() - vertical_offset
        for line in lines:
            if line.y() <= target_y <= line.y() + line.height():
                if x < line.x() - 3 or x > line.x() + line.naturalTextWidth() + 3:
                    return -1
                return line.xToCursor(x, QTextLine.CursorPosition.CursorOnCharacter)
        return -1

    def sizeHint(self):
        base_size = int(self._display.get("base_size", 18))
        ruby_size = int(self._display.get("ruby_size", 9))
        line_spacing = int(self._display.get("line_spacing", 8)) if self._expanded else max(2, int(self._display.get("line_spacing", 8)) // 2)
        if self._furigana:
            ruby_font = _lyric_font(self._display, ruby_size if self._expanded else max(6, ruby_size - 2))
            base_font = _lyric_font(self._display, base_size if self._expanded else max(11, base_size - 4))
            height = QFontMetrics(ruby_font).height() + QFontMetrics(base_font).height() + line_spacing + 14
            return QSize(100, max(44, height))
        base_font = _lyric_font(self._display, base_size if self._expanded else max(11, base_size - 4))
        return QSize(100, max(38, QFontMetrics(base_font).height() + line_spacing + 14))


class LyricsPlayerView(QWidget):
    lyricClicked = pyqtSignal(int)
    wordRequested = pyqtSignal(dict)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("LyricsPlayerView")
        self.setMouseTracking(True)
        self.setMinimumHeight(360)
        self._lyrics: list[tuple[int, str, str]] = []
        self._display = _song_display_settings()
        self._show_furigana = False
        self._position = 0
        self._active_index = 0
        self._visual_index = 0.0
        self._hit_rows: list[tuple[QRectF, int]] = []
        self._hit_words: list[tuple[QRectF, dict]] = []
        self._scroll_anim = None
        self._theme = _active_theme()

    def setTheme(self, theme: str):
        self._theme = "light" if theme == "light" else "dark"
        self.update()

    def setLyrics(self, lyrics: list[tuple[int, str, str]]):
        self._lyrics = list(lyrics or [])
        self._active_index = self._index_for_position(self._position)
        self._visual_index = float(self._active_index)
        self.update()

    def setDisplaySettings(self, display: dict):
        self._display = dict(display)
        self.update()

    def setFuriganaVisible(self, visible: bool):
        self._show_furigana = bool(visible)
        self.update()

    def setPosition(self, position: int, *, force: bool = False):
        self._position = max(0, int(position))
        target = self._index_for_position(self._position)
        if force:
            self._active_index = target
            self._visual_index = float(target)
            self.update()
            return
        if target != self._active_index:
            self._active_index = target
            self._animate_to_index(target)
        else:
            self.update()

    def _index_for_position(self, position: int) -> int:
        if not self._lyrics:
            return 0
        index = 0
        for i, (ms, _, _) in enumerate(self._lyrics):
            if ms <= position:
                index = i
            else:
                break
        return index

    def _animate_to_index(self, target: int):
        if self._scroll_anim:
            self._scroll_anim.stop()
        anim = QPropertyAnimation(self, b"visualIndex", self)
        anim.setDuration(260)
        anim.setStartValue(float(self._visual_index))
        anim.setEndValue(float(target))
        anim.setEasingCurve(QEasingCurve.Type.OutCubic)
        self._scroll_anim = anim
        anim.start()

    def getVisualIndex(self) -> float:
        return float(self._visual_index)

    def setVisualIndex(self, value: float):
        if not self._lyrics:
            self._visual_index = 0.0
        else:
            self._visual_index = max(0.0, min(float(value), len(self._lyrics) - 1))
        self.update()

    visualIndex = pyqtProperty(float, fget=getVisualIndex, fset=setVisualIndex)

    def _fonts(self, active: bool):
        base_size = int(self._display.get("base_size", 18))
        ruby_size = int(self._display.get("ruby_size", 9))
        base_font = _lyric_font(
            self._display,
            base_size + (1 if active else 0),
            QFont.Weight.Bold if active else QFont.Weight.Medium,
        )
        ruby_font = _lyric_font(self._display, ruby_size, QFont.Weight.Medium)
        time_font = _lyric_font(self._display, max(9, ruby_size + 1))
        return ruby_font, base_font, time_font

    def _row_height(self) -> int:
        ruby_font, base_font, _ = self._fonts(False)
        h = QFontMetrics(ruby_font).height() + QFontMetrics(base_font).height()
        return max(58, h + int(self._display.get("line_spacing", 8)) + 22)

    def wheelEvent(self, event):
        if not self._lyrics:
            return
        if self._scroll_anim:
            self._scroll_anim.stop()
        steps = event.angleDelta().y() / 120.0
        self.setVisualIndex(self._visual_index - steps * 0.9)
        event.accept()

    def mousePressEvent(self, event):
        pos = event.position()
        if event.button() == Qt.MouseButton.RightButton:
            for rect, token in self._hit_words:
                if rect.contains(pos):
                    self.wordRequested.emit(token)
                    event.accept()
                    return
        for rect, ms in self._hit_rows:
            if rect.contains(pos):
                self.lyricClicked.emit(ms)
                return
        super().mousePressEvent(event)

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setRenderHint(QPainter.RenderHint.TextAntialiasing, True)
        self._hit_rows.clear()
        self._hit_words.clear()
        colors = _lyric_colors(self._theme)
        painter.fillRect(self.rect(), QColor(colors["bg"]))
        if not self._lyrics:
            painter.setPen(QColor(colors["muted"]))
            painter.drawText(self.rect(), Qt.AlignmentFlag.AlignCenter, "选择歌曲后显示歌词")
            return

        row_h = self._row_height()
        center_y = self.height() * 0.48
        start = max(0, int(self._visual_index - self.height() / row_h - 2))
        end = min(len(self._lyrics), int(self._visual_index + self.height() / row_h + 3))
        for i in range(start, end):
            ms, time_text, lyric = self._lyrics[i]
            active = i == self._active_index
            y = center_y + (i - self._visual_index) * row_h
            rect = QRectF(16, y - row_h / 2 + 4, self.width() - 32, row_h - 8)
            if rect.bottom() < 0 or rect.top() > self.height():
                continue
            self._hit_rows.append((rect, ms))
            if active:
                painter.setPen(Qt.PenStyle.NoPen)
                painter.setBrush(QColor(colors["active_bg"]))
                painter.drawRoundedRect(rect, 10, 10)
            self._draw_line(painter, rect, ms, time_text, lyric, active)

    def _draw_line(self, painter: QPainter, rect: QRectF, ms: int, time_text: str, lyric: str, active: bool):
        ruby_font, base_font, time_font = self._fonts(active)
        ruby_metrics = QFontMetrics(ruby_font)
        base_metrics = QFontMetrics(base_font)
        time_metrics = QFontMetrics(time_font)
        time_x = rect.left() + 22
        text_x = rect.left() + 150
        text_right = rect.right() - 24
        row_center = rect.center().y()
        painter.setFont(time_font)
        colors = _lyric_colors(self._theme)
        painter.setPen(QColor(colors["active_ruby"] if active else colors["time"]))
        painter.drawText(int(time_x), int(row_center + time_metrics.ascent() / 2 - 2), time_text)

        base_color = QColor(colors["active"] if active else colors["text"])
        ruby_color = QColor(colors["active_ruby"] if active else colors["ruby"])
        if not self._show_furigana:
            painter.setFont(base_font)
            painter.setPen(base_color)
            self._draw_token_text(
                painter, lyric, text_x, row_center + base_metrics.ascent() / 2 - 4,
                text_right, base_metrics, rect, ms, time_text
            )
            return

        tokens = _furigana_tokens(lyric, self._display.get("furigana_mode", "kanji"))
        spacing = max(4, int(self._display.get("line_spacing", 8) * 0.35)) + int(self._display.get("letter_spacing", 1))
        base_y = row_center + (ruby_metrics.height() + base_metrics.ascent()) / 2 - 4
        ruby_y = base_y - base_metrics.ascent() - 3
        x = text_x
        for surface, reading in tokens:
            base_w = base_metrics.horizontalAdvance(surface)
            ruby_w = ruby_metrics.horizontalAdvance(reading)
            cell_w = max(base_w, ruby_w)
            if x + cell_w > text_right:
                break
            if reading:
                painter.setFont(ruby_font)
                painter.setPen(ruby_color)
                painter.drawText(int(x + (cell_w - ruby_w) / 2), int(ruby_y), reading)
            painter.setFont(base_font)
            painter.setPen(base_color)
            painter.drawText(int(x + (cell_w - base_w) / 2), int(base_y), surface)
            x += cell_w + spacing
        for left, right, token in _furigana_word_boxes(
            lyric, tokens, base_metrics, ruby_metrics, text_x, spacing, text_right
        ):
            token_rect = QRectF(
                left,
                rect.top() + 4,
                max(4, right - left),
                rect.height() - 8,
            )
            self._hit_words.append((
                token_rect,
                {**token, "lyric": lyric, "time_ms": ms, "time_text": time_text},
            ))

    def _draw_token_text(self, painter: QPainter, text: str, x: float, baseline: float,
                         right: float, metrics: QFontMetrics, row_rect: QRectF, ms: int, time_text: str):
        tokens = _word_tokens(text)
        spacing = int(self._display.get("letter_spacing", 1))
        cursor = x
        for token in tokens:
            surface = token.get("surface", "")
            token_width = sum(metrics.horizontalAdvance(ch) + spacing for ch in surface)
            token_width = max(0, token_width - spacing)
            if cursor + token_width > right:
                painter.drawText(int(cursor), int(baseline), "…")
                break
            draw_x = cursor
            for ch in surface:
                w = metrics.horizontalAdvance(ch)
                painter.drawText(int(draw_x), int(baseline), ch)
                draw_x += w + spacing
            token_rect = QRectF(cursor, row_rect.top() + 4, max(4, token_width), row_rect.height() - 8)
            self._hit_words.append((token_rect, {**token, "lyric": text, "time_ms": ms, "time_text": time_text}))
            cursor += token_width + spacing

class PitchShiftWorker(QThread):
    finished = pyqtSignal(str, int, str, int, bool, str)

    def __init__(self, source: str, output: str, semitones: int, position: int, autoplay: bool, parent=None):
        super().__init__(parent)
        self.source = source
        self.output = output
        self.semitones = int(semitones)
        self.position = int(position)
        self.autoplay = bool(autoplay)

    def run(self):
        try:
            out = pathlib.Path(self.output)
            out.parent.mkdir(parents=True, exist_ok=True)
            if out.exists() and out.stat().st_size > 0:
                self.finished.emit(self.source, self.semitones, self.output, self.position, self.autoplay, "")
                return
            ffmpeg = (
                shutil.which("ffmpeg")
                or (str(app_dir() / "ffmpeg.exe") if (app_dir() / "ffmpeg.exe").exists() else "")
            )
            if not ffmpeg:
                self.finished.emit(self.source, self.semitones, "", self.position, False, "ffmpeg.exe not found")
                return
            ratio = pow(2.0, self.semitones / 12.0)
            cmd = [
                ffmpeg, "-y", "-hide_banner", "-loglevel", "error",
                "-i", self.source,
                "-filter:a", f"rubberband=pitch={ratio:.8f}",
                "-vn", "-c:a", "flac", self.output,
            ]
            result = subprocess.run(cmd, capture_output=True, text=True, timeout=180)
            if result.returncode != 0:
                message = (result.stderr or result.stdout or "pitch shift failed").strip()
                self.finished.emit(self.source, self.semitones, "", self.position, False, message[:500])
                return
            self.finished.emit(self.source, self.semitones, self.output, self.position, self.autoplay, "")
        except Exception as exc:
            self.finished.emit(self.source, self.semitones, "", self.position, False, str(exc))


def _insight_cache_get(cache: OrderedDict, key: tuple) -> dict | None:
    if key not in cache:
        return None
    cache.move_to_end(key)
    return copy.deepcopy(cache[key])


def _insight_cache_put(cache: OrderedDict, key: tuple, value: dict):
    cache[key] = copy.deepcopy(value or {})
    cache.move_to_end(key)
    while len(cache) > _WORD_CACHE_LIMIT:
        cache.popitem(last=False)


def _insight_dict_key(token: dict) -> tuple:
    return (
        token.get("lemma", "") or token.get("surface", ""),
        token.get("surface", ""),
        token.get("pos", ""),
        token.get("reading", ""),
    )


def _insight_corpus_key(token: dict, current_song_id: str) -> tuple:
    return (token.get("lemma", "") or token.get("surface", ""), current_song_id or "")


def _base_insight_data(token: dict) -> dict:
    surface = token.get("surface", "")
    lemma = token.get("lemma", surface)
    return {
        "surface": surface,
        "lemma": lemma,
        "pos": token.get("pos", ""),
        "reading": token.get("reading", ""),
        "lookup_term": lemma,
        "defs": [],
        "pitch": "",
        "freq": "",
        "jlpt": "",
        "occurrences": 0,
        "song_count": 0,
        "examples": [],
        "error": "",
        "_dict_ready": False,
        "_corpus_ready": False,
    }


def _ensure_insight_lookup_indexes():
    global _INSIGHT_INDEXES_READY
    if _INSIGHT_INDEXES_READY:
        return
    try:
        conn = sqlite3.connect(str(DB_PATH))
        conn.execute("CREATE INDEX IF NOT EXISTS idx_tokens_lemma ON tokens(lemma)")
        conn.execute("CREATE INDEX IF NOT EXISTS idx_tokens_lemma_utterance ON tokens(lemma, utterance_id)")
        conn.execute("CREATE INDEX IF NOT EXISTS idx_tokens_utterance ON tokens(utterance_id)")
        conn.execute("CREATE INDEX IF NOT EXISTS idx_utterances_song_time ON utterances(song_id, time_sec)")
        conn.commit()
        conn.close()
    except Exception:
        pass
    _INSIGHT_INDEXES_READY = True


def _lookup_word_dict_fast(token: dict) -> dict:
    key = _insight_dict_key(token)
    cached = _insight_cache_get(_WORD_DICT_CACHE, key)
    if cached is not None:
        return cached

    data = _base_insight_data(token)
    surface = data["surface"]
    lemma = data["lemma"]
    pos = data["pos"]
    try:
        candidates = _lookup_candidates(lemma, pos, surface) if "_lookup_candidates" in globals() else [lemma, surface]
        for term in candidates or [lemma]:
            lookup = _lookup_yomitan_zh(term)
            if lookup.get("defs") or lookup.get("reading"):
                data["lookup_term"] = term
                data["defs"] = lookup.get("defs", [])
                data["reading"] = data["reading"] or lookup.get("reading", "")
                break
        lookup_term = data["lookup_term"] or lemma
        data["pitch"] = (
            _lookup_yomitan_pitch(lookup_term, data["reading"])
            or _lookup_yomitan_pitch(lemma, data["reading"])
        )
        data["freq"] = _lookup_yomitan_freq(lemma) or _lookup_yomitan_freq(lookup_term)
        try:
            conn = sqlite3.connect(str(DB_PATH))
            row = conn.execute("SELECT level FROM jlpt_cache WHERE lemma=?", (lemma,)).fetchone()
            conn.close()
            data["jlpt"] = row[0] if row else ""
        except Exception:
            pass
        data["_dict_ready"] = True
    except Exception as exc:
        data["error"] = str(exc)

    _insight_cache_put(_WORD_DICT_CACHE, key, data)
    return copy.deepcopy(data)


def _lookup_word_corpus_fast(token: dict, current_song_id: str) -> dict:
    key = _insight_corpus_key(token, current_song_id)
    cached = _insight_cache_get(_WORD_CORPUS_CACHE, key)
    if cached is not None:
        return cached

    lemma = token.get("lemma", "") or token.get("surface", "")
    data = {"occurrences": 0, "song_count": 0, "examples": [], "_corpus_ready": True}
    try:
        _ensure_insight_lookup_indexes()
        conn = sqlite3.connect(str(DB_PATH))
        row = conn.execute(
            """
            SELECT COUNT(*), COUNT(DISTINCT u.song_id)
            FROM tokens t
            JOIN utterances u ON u.id = t.utterance_id
            WHERE t.lemma=?
            """,
            (lemma,),
        ).fetchone()
        if row:
            data["occurrences"], data["song_count"] = int(row[0] or 0), int(row[1] or 0)

        rows = conn.execute(
            """
            SELECT s.artist, s.title, u.song_id, u.time_sec, u.text
            FROM tokens t
            JOIN utterances u ON u.id = t.utterance_id
            JOIN songs s ON s.id = u.song_id
            WHERE t.lemma=?
            GROUP BY u.id
            ORDER BY CASE WHEN u.song_id=? THEN 0 ELSE 1 END, s.artist, s.title, u.time_sec
            LIMIT 18
            """,
            (lemma, current_song_id),
        ).fetchall()
        conn.close()
        data["examples"] = [
            {"artist": a or "", "title": title or "", "song_id": sid, "time": float(ts or 0), "text": text or ""}
            for a, title, sid, ts, text in rows
        ]
    except Exception as exc:
        data["error"] = str(exc)

    _insight_cache_put(_WORD_CORPUS_CACHE, key, data)
    return copy.deepcopy(data)


class WordInsightWorker(QThread):
    partialReady = pyqtSignal(dict)
    ready = pyqtSignal(dict)

    def __init__(self, token: dict, current_song_id: str = "", parent=None):
        super().__init__(parent)
        self.token = dict(token or {})
        self.current_song_id = current_song_id or ""

    def run(self):
        full_key = (_insight_dict_key(self.token), _insight_corpus_key(self.token, self.current_song_id))
        cached = _insight_cache_get(_WORD_FULL_CACHE, full_key)
        if cached is not None:
            self.ready.emit(cached)
            return

        data = _lookup_word_dict_fast(self.token)
        self.partialReady.emit(copy.deepcopy(data))
        try:
            corpus = _lookup_word_corpus_fast(self.token, self.current_song_id)
            data.update(corpus)
        except Exception as exc:
            data["error"] = str(exc)
            data["_corpus_ready"] = True
        _insight_cache_put(_WORD_FULL_CACHE, full_key, data)
        self.ready.emit(data)


class QuickAnkiAddWorker(QThread):
    finished = pyqtSignal(bool, str)

    def __init__(self, data: dict, parent=None):
        super().__init__(parent)
        self.data = dict(data or {})

    def run(self):
        try:
            _ensure_anki_model()
            cfg = load_settings().get("anki_dialog", {})
            deck = (self.data.get("deck") or cfg.get("deck") or "JPOP Corpus").strip() or "JPOP Corpus"
            decks = _anki_request("deckNames") or []
            if deck not in decks:
                _anki_request("createDeck", deck=deck)

            lemma = self.data.get("lemma") or self.data.get("surface") or ""
            if not lemma:
                self.finished.emit(False, "没有可发送的词。")
                return
            reading = self.data.get("reading", "")
            lookup_term = self.data.get("lookup_term", lemma)
            meaning = _prefix_lookup_term_html(
                _build_meaning_html(self.data.get("defs") or []),
                lemma,
                lookup_term,
            )
            examples = self.data.get("selected_examples") or self.data.get("examples") or []
            sent_parts = []
            sources = []
            for ex in examples[:8]:
                text = ex.get("text", "")
                target = self.data.get("surface") or lemma
                if target and target in text:
                    parts = text.split(target, 1)
                    text = (
                        html_lib.escape(parts[0])
                        + f'<b style="color:#c0392b">{html_lib.escape(target)}</b>'
                        + html_lib.escape(parts[1])
                    )
                else:
                    text = html_lib.escape(text)
                mins = int(float(ex.get("time", 0)) // 60)
                secs = float(ex.get("time", 0)) % 60
                source = f"{ex.get('artist', '')}「{ex.get('title', '')}」"
                sources.append(source)
                sent_parts.append(
                    f'<div class="sent">{text}'
                    f'<span class="sent-src">{html_lib.escape(source)} {mins:02d}:{secs:04.1f}</span></div>'
                )

            fields = {
                "Expression": lemma,
                "Reading": reading,
                "Meaning": meaning,
                "Sentence": "".join(sent_parts),
                "SentenceAudio": "",
                "Source": "、".join(dict.fromkeys(sources)),
                "JLPT": self.data.get("jlpt", ""),
                "Pitch": self.data.get("pitch", ""),
                "Freq": str(self.data.get("freq", "")),
                "PartOfSpeech": _POS_JA.get(self.data.get("pos", ""), self.data.get("pos", "")),
            }
            query = f'deck:"{deck}" note:"{_ANKI_MODEL}" Expression:"{lemma}"'
            existing = _anki_request("findNotes", query=query) or []
            if existing:
                info = (_anki_request("notesInfo", notes=[existing[0]]) or [{}])[0]
                old_fields = info.get("fields", {})
                old_sentence = old_fields.get("Sentence", {}).get("value", "")
                new_sentence = "".join(part for part in sent_parts if part not in old_sentence)
                update_fields = dict(fields)
                update_fields["Sentence"] = old_sentence + new_sentence
                _anki_request("updateNoteFields", note={"id": existing[0], "fields": update_fields})
                self.finished.emit(True, f"已更新旧卡：{lemma}")
                return

            note = {
                "deckName": deck,
                "modelName": _ANKI_MODEL,
                "fields": fields,
                "options": {"allowDuplicate": False, "duplicateScope": "deck"},
                "tags": ["jpop-corpus", "song-player"],
            }
            _anki_request("addNote", note=note)
            self.finished.emit(True, f"已加入 Anki：{lemma} → {deck}")
        except Exception as exc:
            self.finished.emit(False, str(exc))


class DeckListWorker(QThread):
    ready = pyqtSignal(list, str)

    def run(self):
        try:
            decks = _anki_request("deckNames") or []
            self.ready.emit(sorted(str(d) for d in decks), "")
        except Exception as exc:
            self.ready.emit([], str(exc))


class WordInsightPanel(QFrame):
    addToAnkiRequested = pyqtSignal(dict)
    searchCorpusRequested = pyqtSignal(str)
    refreshDecksRequested = pyqtSignal()

    def __init__(self, lang: str, parent=None):
        super().__init__(parent)
        self.lang = lang
        self.setObjectName("WordInsightPanel")
        self._data = {}
        self._syncing_examples = False
        self._theme = _active_theme()
        self._build_ui()

    def _text(self, zh: str, ja: str) -> str:
        return zh if self.lang == "zh" else ja

    def _build_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(18, 16, 18, 16)
        layout.setSpacing(12)

        header = QHBoxLayout()
        title_box = QVBoxLayout()
        title_box.setSpacing(2)
        self.lbl_word = QLabel(self._text("选择词语", "語を選択"))
        self.lbl_word.setObjectName("InsightWord")
        self.lbl_meta = QLabel("")
        self.lbl_meta.setObjectName("InsightMeta")
        title_box.addWidget(self.lbl_word)
        title_box.addWidget(self.lbl_meta)
        header.addLayout(title_box, 1)
        self.btn_close = QPushButton("×")
        self.btn_close.setFixedSize(34, 34)
        set_button_role(self.btn_close, "subtle")
        header.addWidget(self.btn_close)
        layout.addLayout(header)

        self.lbl_stats = QLabel("")
        self.lbl_stats.setObjectName("InsightStats")
        self.lbl_stats.setWordWrap(True)
        layout.addWidget(self.lbl_stats)

        switch_row = QHBoxLayout()
        switch_row.setContentsMargins(0, 0, 0, 0)
        switch_row.setSpacing(8)
        self.btn_defs_view = QPushButton(self._text("释义", "語義"))
        self.btn_defs_view.setObjectName("InsightSegment")
        self.btn_defs_view.setCheckable(True)
        self.btn_defs_view.setChecked(True)
        self.btn_examples_view = QPushButton(self._text("语料出现", "コーパス例"))
        self.btn_examples_view.setObjectName("InsightSegment")
        self.btn_examples_view.setCheckable(True)
        self.btn_defs_view.clicked.connect(lambda: self._set_view_mode("defs"))
        self.btn_examples_view.clicked.connect(lambda: self._set_view_mode("examples"))
        switch_row.addWidget(self.btn_defs_view)
        switch_row.addWidget(self.btn_examples_view)
        switch_row.addStretch()
        layout.addLayout(switch_row)

        self.content_stack = QStackedWidget()
        self.browser = QTextBrowser()
        self.browser.setObjectName("InsightBrowser")
        self.browser.setOpenExternalLinks(False)
        self.content_stack.addWidget(self.browser)
        self.examples_list = QListWidget()
        self.examples_list.setObjectName("InsightExamples")
        self.examples_list.setAlternatingRowColors(False)
        self.examples_list.itemChanged.connect(self._on_example_item_changed)
        self.content_stack.addWidget(self.examples_list)
        layout.addWidget(self.content_stack, 1)

        deck_row = QHBoxLayout()
        deck_row.setContentsMargins(0, 0, 0, 0)
        deck_row.setSpacing(8)
        deck_label = QLabel(self._text("牌组", "デッキ"))
        deck_label.setObjectName("InsightMeta")
        deck_row.addWidget(deck_label)
        self.deck_combo = QComboBox()
        self.deck_combo.setObjectName("InsightDeckCombo")
        self.deck_combo.setEditable(True)
        self.deck_combo.setMinimumHeight(34)
        default_deck = load_settings().get("anki_dialog", {}).get("deck", "") or "JPOP Corpus"
        self.deck_combo.addItem(default_deck)
        self.deck_combo.setCurrentText(default_deck)
        deck_row.addWidget(self.deck_combo, 1)
        self.btn_refresh_decks = QPushButton(self._text("刷新", "更新"))
        self.btn_refresh_decks.setObjectName("InsightRefreshDecks")
        set_button_role(self.btn_refresh_decks, "subtle")
        self.btn_refresh_decks.clicked.connect(self.refreshDecksRequested.emit)
        deck_row.addWidget(self.btn_refresh_decks)
        layout.addLayout(deck_row)

        action_row = QHBoxLayout()
        action_row.setSpacing(8)
        self.btn_anki = QPushButton(self._text("加入 Anki", "Anki に追加"))
        set_button_role(self.btn_anki, "primary")
        self.btn_anki.clicked.connect(lambda: self.addToAnkiRequested.emit(self.current_data()))
        action_row.addWidget(self.btn_anki)
        self.btn_corpus = QPushButton(self._text("查看语料", "コーパスを見る"))
        set_button_role(self.btn_corpus, "subtle")
        self.btn_corpus.clicked.connect(lambda: self.searchCorpusRequested.emit(self._data.get("lemma", "")))
        action_row.addWidget(self.btn_corpus)
        layout.addLayout(action_row)

    def selected_deck(self) -> str:
        return (self.deck_combo.currentText() or "").strip() or "JPOP Corpus"

    def set_decks(self, decks: list[str], selected: str = ""):
        current = (selected or self.selected_deck()).strip() or "JPOP Corpus"
        unique_decks = []
        seen = set()
        for deck in decks or []:
            name = str(deck).strip()
            if not name or name in seen:
                continue
            unique_decks.append(name)
            seen.add(name)
        if current not in seen:
            unique_decks.insert(0, current)
        self.deck_combo.blockSignals(True)
        self.deck_combo.clear()
        self.deck_combo.addItems(unique_decks or [current])
        self.deck_combo.setCurrentText(current)
        self.deck_combo.blockSignals(False)

    def _set_view_mode(self, mode: str):
        examples = mode == "examples"
        self.btn_defs_view.blockSignals(True)
        self.btn_examples_view.blockSignals(True)
        self.btn_defs_view.setChecked(not examples)
        self.btn_examples_view.setChecked(examples)
        self.btn_defs_view.blockSignals(False)
        self.btn_examples_view.blockSignals(False)
        self.content_stack.setCurrentIndex(1 if examples else 0)

    def setTheme(self, theme: str):
        self._theme = "light" if theme == "light" else "dark"
        if self._data.get("_dict_ready"):
            self.browser.setHtml(self._build_defs_html(self._data))
        elif self._data:
            self.browser.setHtml(self._loading_html())

    def _loading_html(self) -> str:
        colors = _lyric_colors(self._theme)
        message = self._text("正在查询本地词典…", "ローカル辞書を検索中…")
        return (
            "<html><body style='margin:0;background:transparent;'>"
            f"<div style='color:{colors['muted']};font-size:14px;padding:12px'>"
            f"{html_lib.escape(message)}</div></body></html>"
        )

    def set_loading(self, token: dict):
        self._data = dict(token or {})
        self._data["_dict_ready"] = False
        self._data["_corpus_ready"] = False
        surface = token.get("surface", "")
        lemma = token.get("lemma", surface)
        self.lbl_word.setText(html_lib.escape(surface or lemma))
        self.lbl_meta.setText(html_lib.escape(lemma if lemma != surface else token.get("pos", "")))
        self.lbl_stats.setText(self._text("正在查词…", "辞書を検索中…"))
        self._set_view_mode("defs")
        self.browser.setHtml(self._loading_html())
        self._syncing_examples = True
        self.examples_list.clear()
        item = QListWidgetItem(self._text("语料出现稍后补全…", "コーパス例は後から補完されます…"))
        item.setFlags(Qt.ItemFlag.NoItemFlags)
        self.examples_list.addItem(item)
        self._syncing_examples = False

    def set_data(self, data: dict, *, update_examples: bool = True):
        merged = dict(self._data)
        merged.update(data or {})
        if not update_examples and self._data.get("selected_examples"):
            merged["selected_examples"] = self._data["selected_examples"]
        self._data = merged
        surface = merged.get("surface", "")
        lemma = merged.get("lemma", surface)
        lookup_term = merged.get("lookup_term", lemma)
        self.lbl_word.setText(html_lib.escape(surface or lemma))
        meta_parts = [p for p in [
            lemma if lemma != surface else "",
            merged.get("reading", ""),
            merged.get("pos", ""),
            merged.get("jlpt", ""),
        ] if p]
        self.lbl_meta.setText(" / ".join(html_lib.escape(str(p)) for p in meta_parts))
        stat_bits = []
        if merged.get("_corpus_ready"):
            stat_bits.append(
                self._text(f"语料 {merged.get('occurrences', 0):,} 次 / {merged.get('song_count', 0):,} 首歌",
                           f"コーパス {merged.get('occurrences', 0):,} 回 / {merged.get('song_count', 0):,} 曲")
            )
        else:
            stat_bits.append(self._text("语料查询中…", "コーパス検索中…"))
        if merged.get("freq"):
            stat_bits.append(f"JPDB {html_lib.escape(str(merged.get('freq')))}")
        if merged.get("pitch"):
            stat_bits.append(html_lib.escape(str(merged.get("pitch"))))
        if lookup_term and lookup_term != lemma:
            stat_bits.append(self._text(f"查词：{lookup_term}", f"辞書形：{lookup_term}"))
        self.lbl_stats.setText(" · ".join(stat_bits))
        self.browser.setHtml(self._build_defs_html(merged))
        if update_examples:
            self._populate_examples(merged.get("examples") or [])

    def _build_defs_html(self, data: dict) -> str:
        colors = _lyric_colors(self._theme)
        accent = "#0891b2" if self._theme == "light" else "#67e8f9"
        border = "#d8e0ea" if self._theme == "light" else "#343c48"
        css = f"""
        <style>
        body{{font-family:'Microsoft YaHei UI','Yu Gothic UI',sans-serif;background:transparent;color:{colors['text']};font-size:14px;margin:0}}
        .group{{border-top:1px solid {border};padding:10px 0 8px}}
        .group:first-child{{border-top:0;padding-top:0}}
        .src{{color:{accent};font-weight:700;margin-bottom:6px;font-size:15px}}
        .def{{line-height:1.65;color:{colors['text']};margin:0 0 7px}}
        .muted{{color:{colors['muted']}}}
        </style>
        """
        parts = [css]
        if data.get("error"):
            parts.append(f"<div class='def'>{html_lib.escape(data['error'])}</div>")
        defs = data.get("defs") or []
        if defs:
            grouped = {}
            for src, text in defs[:16]:
                grouped.setdefault(str(src), []).append(_plain_def_text(text))
            for src, items in grouped.items():
                parts.append("<div class='group'>")
                parts.append(f"<div class='src'>{html_lib.escape(src)}</div>")
                for text in items:
                    parts.append(f"<div class='def'>{html_lib.escape(text)}</div>")
                parts.append("</div>")
        else:
            parts.append(f"<div class='muted'>{self._text('本地词典暂无释义', 'ローカル辞書に語義がありません')}</div>")
        return "".join(parts)

    def _populate_examples(self, examples: list):
        self._syncing_examples = True
        self.examples_list.clear()
        if examples:
            for index, ex in enumerate(examples):
                mins = int(float(ex.get("time", 0)) // 60)
                secs = float(ex.get("time", 0)) % 60
                song = f"{ex.get('artist', '')} - {ex.get('title', '')}  {mins:02d}:{secs:05.2f}"
                item = QListWidgetItem(f"{song}\n{ex.get('text', '')}")
                item.setData(Qt.ItemDataRole.UserRole, dict(ex))
                item.setFlags(item.flags() | Qt.ItemFlag.ItemIsUserCheckable)
                item.setCheckState(Qt.CheckState.Checked if index < 2 else Qt.CheckState.Unchecked)
                item.setSizeHint(QSize(120, 70))
                self.examples_list.addItem(item)
        else:
            item = QListWidgetItem(self._text("语料库中暂无例句", "コーパス例はありません"))
            item.setFlags(Qt.ItemFlag.NoItemFlags)
            self.examples_list.addItem(item)
        self._syncing_examples = False
        self._on_example_item_changed(None)

    def _on_example_item_changed(self, item):
        if self._syncing_examples:
            return
        self._data["selected_examples"] = self.selected_examples()

    def selected_examples(self) -> list:
        selected = []
        for i in range(self.examples_list.count()):
            item = self.examples_list.item(i)
            if item and item.checkState() == Qt.CheckState.Checked:
                data = item.data(Qt.ItemDataRole.UserRole)
                if data:
                    selected.append(data)
        return selected

    def current_data(self) -> dict:
        data = dict(self._data)
        data["selected_examples"] = self.selected_examples()
        data["deck"] = self.selected_deck()
        return data


class SongDisplaySettingsDialog(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        load_custom_fonts(load_settings().get("custom_font_paths", []))
        self.lang = load_settings().get("ui_language", "zh")
        self._cfg = load_settings()
        self._custom_font_paths = list(self._cfg.get("custom_font_paths", []))
        self.setWindowTitle("曲库显示设置" if self.lang == "zh" else "曲庫表示設定")
        self.setMinimumWidth(640)
        self._apply_local_style()

        layout = QVBoxLayout(self)
        layout.setContentsMargins(24, 22, 24, 22)
        layout.setSpacing(14)

        title = QLabel("曲库显示设置" if self.lang == "zh" else "曲庫表示設定")
        title.setObjectName("SettingsTitle")
        layout.addWidget(title)
        subtitle = QLabel(
            "调整曲库播放器里的歌词字体、字号、行距和字间距"
            if self.lang == "zh"
            else "曲庫プレイヤーの歌詞フォント、サイズ、行間、字間を調整します"
        )
        subtitle.setObjectName("SettingsSubtitle")
        layout.addWidget(subtitle)

        form_panel = QFrame()
        form_panel.setObjectName("SettingsPanel")
        form = QVBoxLayout(form_panel)
        form.setContentsMargins(18, 16, 18, 16)
        form.setSpacing(14)

        family = self._cfg.get("song_lyric_font_family") or self._cfg.get("font_family") or "Klee One"
        fallback_family = self._cfg.get("song_lyric_fallback_font_family") or "LXGW WenKai"
        self.font_combo = self._make_font_combo(family)
        font_label = QLabel("歌词主字体" if self.lang == "zh" else "歌詞メインフォント")
        font_label.setObjectName("SettingsRowLabel")
        font_row = QHBoxLayout()
        font_row.setContentsMargins(0, 0, 0, 0)
        font_row.setSpacing(10)
        btn_import = QPushButton("导入字体" if self.lang == "zh" else "フォント追加")
        set_button_role(btn_import, "subtle")
        btn_import.clicked.connect(self._import_font)
        font_row.addWidget(font_label)
        font_row.addWidget(self.font_combo, 1)
        font_row.addWidget(btn_import)
        form.addLayout(font_row)

        self.fallback_font_combo = self._make_font_combo(fallback_family)
        fallback_label = QLabel("备用字体" if self.lang == "zh" else "予備フォント")
        fallback_label.setObjectName("SettingsRowLabel")
        fallback_row = QHBoxLayout()
        fallback_row.setContentsMargins(0, 0, 0, 0)
        fallback_row.setSpacing(10)
        fallback_row.addWidget(fallback_label)
        fallback_row.addWidget(self.fallback_font_combo, 1)
        form.addLayout(fallback_row)

        mode_label = QLabel("振假名方式" if self.lang == "zh" else "ふりがな方式")
        mode_label.setObjectName("SettingsRowLabel")
        mode_row = QHBoxLayout()
        mode_row.setContentsMargins(0, 0, 0, 0)
        mode_row.setSpacing(10)
        self.furigana_mode_combo = QComboBox()
        self.furigana_mode_combo.setObjectName("SettingsCombo")
        self.furigana_mode_combo.addItem(
            "仅汉字振假名" if self.lang == "zh" else "漢字のみ",
            "kanji",
        )
        self.furigana_mode_combo.addItem(
            "整词振假名（旧）" if self.lang == "zh" else "単語全体（旧）",
            "word",
        )
        current_mode = self._cfg.get("furigana_mode", "kanji")
        self.furigana_mode_combo.setCurrentIndex(max(0, self.furigana_mode_combo.findData(current_mode)))
        mode_row.addWidget(mode_label)
        mode_row.addWidget(self.furigana_mode_combo, 1)
        form.addLayout(mode_row)

        self.base_spin = self._add_slider_row(
            form,
            "歌词字号" if self.lang == "zh" else "歌詞サイズ",
            12,
            30,
            _clamp_int(self._cfg.get("song_lyric_font_size"), 18, 12, 30),
            " pt",
        )
        self.ruby_spin = self._add_slider_row(
            form,
            "振假名字号" if self.lang == "zh" else "ふりがなサイズ",
            6,
            18,
            _clamp_int(self._cfg.get("song_furigana_font_size"), 9, 6, 18),
            " pt",
        )
        self.spacing_spin = self._add_slider_row(
            form,
            "歌词行距" if self.lang == "zh" else "歌詞行間",
            0,
            36,
            _clamp_int(self._cfg.get("song_lyric_line_spacing"), 8, 0, 36),
            " px",
        )
        self.letter_spin = self._add_slider_row(
            form,
            "字间距" if self.lang == "zh" else "字間",
            0,
            12,
            _clamp_int(self._cfg.get("song_lyric_letter_spacing"), 1, 0, 12),
            " px",
        )

        layout.addWidget(form_panel)

        preview_title = QLabel("预览" if self.lang == "zh" else "プレビュー")
        preview_title.setObjectName("SettingsRowLabel")
        layout.addWidget(preview_title)
        self.preview = QLabel("忘れたい自分に缶コーヒーを買った")
        self.preview.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.preview.setMinimumHeight(88)
        self.preview.setObjectName("SettingsPreview")
        layout.addWidget(self.preview)
        for widget in (
            self.font_combo,
            self.fallback_font_combo,
            self.furigana_mode_combo,
            self.base_spin,
            self.ruby_spin,
            self.spacing_spin,
            self.letter_spin,
        ):
            if hasattr(widget, "valueChanged"):
                widget.valueChanged.connect(self._update_preview)
            if hasattr(widget, "currentIndexChanged"):
                widget.currentIndexChanged.connect(self._update_preview)
        self._update_preview()

        btns = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel
        )
        btns.button(QDialogButtonBox.StandardButton.Ok).setText("保存" if self.lang == "zh" else "保存")
        btns.button(QDialogButtonBox.StandardButton.Cancel).setText("取消" if self.lang == "zh" else "キャンセル")
        btns.accepted.connect(self.accept)
        btns.rejected.connect(self.reject)
        layout.addWidget(btns)

    def _font_options(self) -> list[str]:
        preferred = [
            "Klee One",
            "LXGW WenKai",
            "Microsoft YaHei UI",
            "Yu Gothic UI",
            "Meiryo UI",
            "Meiryo",
            "Segoe UI",
            "Noto Sans CJK SC",
            "Noto Sans CJK JP",
            "Source Han Sans SC",
            "Source Han Sans JP",
            "BIZ UDPGothic",
        ]
        installed = set(QFontDatabase.families())
        options = []
        for family in preferred:
            options.append(family)
        for family in sorted(installed, key=str.casefold):
            if family not in options:
                options.append(family)
        return options

    def _make_font_combo(self, current: str) -> QComboBox:
        combo = QComboBox()
        combo.setObjectName("SettingsCombo")
        combo.setMaxVisibleItems(18)
        combo.setMinimumContentsLength(18)
        for family in self._font_options():
            combo.addItem(family, family)
        self._set_font_combo_value(combo, current)
        return combo

    def _set_font_combo_value(self, combo: QComboBox, family: str):
        family = (family or "").strip() or "Klee One"
        idx = combo.findData(family)
        if idx < 0:
            combo.insertItem(0, family, family)
            idx = 0
        combo.setCurrentIndex(idx)

    def _font_combo_value(self, combo: QComboBox) -> str:
        return (combo.currentData() or combo.currentText() or "").strip() or "Klee One"

    def _add_slider_row(self, layout: QVBoxLayout, label_text: str, minimum: int, maximum: int, value: int, suffix: str) -> QSpinBox:
        row = QFrame()
        row.setObjectName("SettingsRow")
        row_layout = QHBoxLayout(row)
        row_layout.setContentsMargins(0, 0, 0, 0)
        row_layout.setSpacing(12)

        label = QLabel(label_text)
        label.setObjectName("SettingsRowLabel")
        label.setMinimumWidth(96)
        row_layout.addWidget(label)

        slider = QSlider(Qt.Orientation.Horizontal)
        slider.setObjectName("SettingsSlider")
        slider.setRange(minimum, maximum)
        slider.setValue(value)
        row_layout.addWidget(slider, 1)

        spin = QSpinBox()
        spin.setObjectName("SettingsValue")
        spin.setRange(minimum, maximum)
        spin.setSuffix(suffix)
        spin.setValue(value)
        spin.setFixedWidth(92)
        row_layout.addWidget(spin)

        slider.valueChanged.connect(spin.setValue)
        spin.valueChanged.connect(slider.setValue)
        layout.addWidget(row)
        return spin

    def _apply_local_style(self, theme: str | None = None):
        theme = theme or self._cfg.get("theme", "dark")
        if theme == "light":
            bg, panel, panel2, fg, muted, border, accent = (
                "#f5f7fa", "#ffffff", "#f8fafc", "#172033", "#64748b", "#d8e0ea", "#1f6fd1"
            )
        else:
            bg, panel, panel2, fg, muted, border, accent = (
                "#1f2329", "#232933", "#1b1f25", "#edf4ff", "#9aa6b5", "#343c48", "#2f80ed"
            )
        self.setStyleSheet(f"""
            QDialog {{
                background: {bg};
                color: {fg};
            }}
            QLabel#SettingsTitle {{
                font-size: 20px;
                font-weight: 750;
                color: {fg};
            }}
            QLabel#SettingsSubtitle {{
                color: {muted};
                font-size: 13px;
            }}
            QFrame#SettingsPanel {{
                background: {panel};
                border: 1px solid {border};
                border-radius: 12px;
            }}
            QLabel#SettingsPreview {{
                background: {panel2};
                border: 1px solid {border};
                border-radius: 12px;
                color: {fg};
                padding: 12px;
            }}
            QLabel#SettingsRowLabel {{
                color: {muted};
                font-size: 13px;
                font-weight: 600;
            }}
            QFrame#SettingsRow {{
                background: transparent;
            }}
            QComboBox#SettingsCombo, QSpinBox#SettingsValue {{
                min-height: 34px;
                border: 1px solid {border};
                border-radius: 17px;
                padding: 4px 10px;
                background: {panel2};
                color: {fg};
            }}
            QComboBox#SettingsCombo:focus, QSpinBox#SettingsValue:focus {{
                border: 1px solid {accent};
            }}
            QSlider#SettingsSlider::groove:horizontal {{
                height: 6px;
                border-radius: 3px;
                background: {border};
            }}
            QSlider#SettingsSlider::sub-page:horizontal {{
                height: 6px;
                border-radius: 3px;
                background: {accent};
            }}
            QSlider#SettingsSlider::handle:horizontal {{
                width: 18px;
                height: 18px;
                margin: -6px 0;
                border-radius: 9px;
                background: #f8fbff;
                border: 3px solid {accent};
            }}
            QPushButton {{
                min-height: 34px;
                border: 1px solid {border};
                border-radius: 17px;
                padding: 4px 14px;
                background: {panel2};
                color: {fg};
            }}
            QPushButton[role="primary"] {{
                background: {accent};
                border-color: {accent};
                color: white;
                font-weight: 650;
            }}
            QPushButton:hover {{
                border-color: {accent};
            }}
            QDialogButtonBox QPushButton {{
                min-width: 88px;
            }}
        """)

    def refresh_theme(self, theme: str):
        self._cfg["theme"] = "light" if theme == "light" else "dark"
        self._apply_local_style(self._cfg["theme"])

    def _import_font(self):
        paths, _ = QFileDialog.getOpenFileNames(
            self,
            "导入字体" if self.lang == "zh" else "フォント追加",
            "",
            "字体文件 (*.ttf *.otf *.ttc);;所有文件 (*)",
        )
        for path in paths:
            if path not in self._custom_font_paths:
                self._custom_font_paths.append(path)
            font_id = QFontDatabase.addApplicationFont(path)
            families = QFontDatabase.applicationFontFamilies(font_id) if font_id >= 0 else []
            if families:
                for family in families:
                    if self.font_combo.findData(family) < 0:
                        self.font_combo.addItem(family, family)
                    if self.fallback_font_combo.findData(family) < 0:
                        self.fallback_font_combo.addItem(family, family)
                self._set_font_combo_value(self.font_combo, families[0])

    def _update_preview(self):
        display = {
            "family": self._font_combo_value(self.font_combo),
            "fallback_family": self._font_combo_value(self.fallback_font_combo),
        }
        font = _lyric_font(display, self.base_spin.value(), QFont.Weight.Medium)
        font.setLetterSpacing(QFont.SpacingType.AbsoluteSpacing, self.letter_spin.value())
        self.preview.setFont(font)
        mode_text = (
            "仅汉字" if self.furigana_mode_combo.currentData() == "kanji" else "整词"
        ) if self.lang == "zh" else (
            "漢字のみ" if self.furigana_mode_combo.currentData() == "kanji" else "単語全体"
        )
        self.preview.setText(
            f"主字体 {self._font_combo_value(self.font_combo)} / 备用 {self._font_combo_value(self.fallback_font_combo)} / 歌词 {self.base_spin.value()}pt / 振假名 {self.ruby_spin.value()}pt / {mode_text}"
            if self.lang == "zh"
            else f"メイン {self._font_combo_value(self.font_combo)} / 予備 {self._font_combo_value(self.fallback_font_combo)} / 歌詞 {self.base_spin.value()}pt / ふりがな {self.ruby_spin.value()}pt / {mode_text}"
        )

    def result_settings(self) -> dict:
        return {
            "song_lyric_font_family": self._font_combo_value(self.font_combo),
            "song_lyric_fallback_font_family": self._font_combo_value(self.fallback_font_combo),
            "song_lyric_font_size": self.base_spin.value(),
            "song_furigana_font_size": self.ruby_spin.value(),
            "song_lyric_line_spacing": self.spacing_spin.value(),
            "song_lyric_letter_spacing": self.letter_spin.value(),
            "furigana_mode": self.furigana_mode_combo.currentData() or "kanji",
            "custom_font_paths": self._custom_font_paths,
        }


class SongEditDialog(QDialog):
    def __init__(self, song: tuple, parent=None):
        # song: (id, title, artist, year, album, genre, audio_path)
        super().__init__(parent)
        self.setWindowTitle("曲を編集")
        self.setMinimumWidth(500)
        _, title, artist, year, album, genre, audio_path = song
        self._audio_path = audio_path or ""

        layout = QVBoxLayout(self)
        form = QFormLayout()
        self.e_title  = QLineEdit(title)
        self.e_artist = QLineEdit(artist)
        self.e_year   = QLineEdit(year or "")
        self.e_album  = QLineEdit(album or "")
        self.e_genre  = QLineEdit(genre or "")
        form.addRow("曲名 *：",   self.e_title)
        form.addRow("歌手 *：",   self.e_artist)
        form.addRow("年：",       self.e_year)
        form.addRow("アルバム：", self.e_album)
        form.addRow("ジャンル：", self.e_genre)

        audio_row = QHBoxLayout()
        self.lbl_audio = QLabel(pathlib.Path(audio_path).name if audio_path else "（未設定）")
        self.lbl_audio.setWordWrap(True)
        btn_pick = QPushButton("変更…")
        btn_pick.clicked.connect(self._pick)
        audio_row.addWidget(self.lbl_audio, 1)
        audio_row.addWidget(btn_pick)
        form.addRow("音声ファイル：", audio_row)
        layout.addLayout(form)

        btns = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel
        )
        btns.accepted.connect(self._validate)
        btns.rejected.connect(self.reject)
        layout.addWidget(btns)

    def _pick(self):
        p, _ = QFileDialog.getOpenFileName(
            self, "音声ファイルを選択", "",
            "音声ファイル (*.flac *.mp3 *.wav *.m4a);;すべてのファイル (*)"
        )
        if p:
            self._audio_path = p
            self.lbl_audio.setText(pathlib.Path(p).name)

    def _validate(self):
        if not self.e_title.text().strip():
            QMessageBox.warning(self, "入力エラー", "曲名を入力してください"); return
        if not self.e_artist.text().strip():
            QMessageBox.warning(self, "入力エラー", "歌手を入力してください"); return
        self.accept()

    def get_data(self) -> dict:
        return {"title": self.e_title.text().strip(), "artist": self.e_artist.text().strip(),
                "year": self.e_year.text().strip(),   "album":  self.e_album.text().strip(),
                "genre": self.e_genre.text().strip(),  "audio_path": self._audio_path}


class SongManagerDialog(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.lang = load_settings().get("ui_language", "zh")
        self.setWindowTitle("曲库" if self.lang == "zh" else "曲管理")
        self.setMinimumWidth(820)
        self.setMinimumHeight(520)
        self._songs: list[dict] = []
        self._song_by_id: dict[str, dict] = {}
        self._current_song_id = ""
        self._audio_output = QAudioOutput(self)
        self._audio_output.setVolume(1.0)
        self._player = QMediaPlayer(self)
        self._player.setAudioOutput(self._audio_output)
        self._playback_rate = normalize_playback_rate(load_settings().get("playback_rate", 1.0))
        self._player.setPlaybackRate(self._playback_rate)
        self._player.playbackStateChanged.connect(self._sync_play_button)
        self._player.positionChanged.connect(self._on_position_changed)
        self._player.durationChanged.connect(self._on_duration_changed)
        cfg = load_settings()
        self._theme = "light" if cfg.get("theme") == "light" else "dark"
        self._panel_anim = None
        self._duration_ms = 0
        self._slider_dragging = False
        self._lyric_rows: list[tuple[int, QListWidgetItem]] = []
        self._expanded_lyric_rows: list[tuple[int, QListWidgetItem]] = []
        self._lyric_data: list[tuple[int, str, str]] = []
        self._current_lyric_item = None
        self._current_expanded_lyric_item = None
        self._show_furigana = bool(load_settings().get("show_furigana", False))
        self._song_display = _song_display_settings()
        self._pitch_semitones = _clamp_int(cfg.get("song_pitch_semitones"), 0, -6, 6)
        self._pitch_worker = None
        self._pitch_processing = False
        self._insight_worker = None
        self._insight_workers = []
        self._deck_worker = None
        self._deck_list_requested = False
        self._anki_quick_worker = None
        self._active_insight_panel = None
        self._insight_request_id = 0
        self._playback_settings_open = False
        self._playback_settings_anim = None
        self._expanded_player_open = False
        self._current_original_audio_path = ""
        self._current_audio_path = ""
        self._build_ui()
        self._load()

    def _text(self, zh: str, ja: str) -> str:
        return zh if self.lang == "zh" else ja

    def _build_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(10)
        self._apply_local_style()

        toolbar = QHBoxLayout()
        toolbar.setContentsMargins(0, 0, 0, 0)
        toolbar.setSpacing(8)
        self.search_edit = QLineEdit()
        self.search_edit.setPlaceholderText(self._text("搜索歌手、歌曲、年份…", "歌手、曲名、年を検索…"))
        self.search_edit.textChanged.connect(self._rebuild_tree)
        toolbar.addWidget(self.search_edit, 1)
        self.chk_group = QCheckBox(self._text("按歌手折叠", "歌手ごとに折りたたむ"))
        self.chk_group.setChecked(True)
        self.chk_group.toggled.connect(self._rebuild_tree)
        toolbar.addWidget(self.chk_group)
        self.btn_song_display_settings = QPushButton(self._text("显示设置", "表示設定"))
        set_button_role(self.btn_song_display_settings, "subtle")
        self.btn_song_display_settings.clicked.connect(self._open_display_settings)
        toolbar.addWidget(self.btn_song_display_settings)
        self.lbl_count = QLabel("")
        toolbar.addWidget(self.lbl_count)
        layout.addLayout(toolbar)

        splitter = QSplitter(Qt.Orientation.Horizontal)
        splitter.setChildrenCollapsible(False)
        layout.addWidget(splitter, 1)

        left = QWidget()
        left_layout = QVBoxLayout(left)
        left_layout.setContentsMargins(0, 0, 0, 0)
        left_layout.setSpacing(8)
        self.tree = QTreeWidget()
        self.tree.setColumnCount(5)
        self.tree.setHeaderLabels(
            ["ID", "歌手 / 曲名", self._text("年份", "年"), self._text("行数", "行数"), ""]
        )
        self.tree.setRootIsDecorated(True)
        self.tree.setUniformRowHeights(True)
        self.tree.setAlternatingRowColors(False)
        self.tree.setSelectionMode(QTreeWidget.SelectionMode.SingleSelection)
        self.tree.itemSelectionChanged.connect(self._on_selection_changed)
        self.tree.itemDoubleClicked.connect(lambda *_: self._toggle_play())
        self.tree.setColumnWidth(0, 64)
        self.tree.setColumnWidth(1, 360)
        self.tree.setColumnWidth(2, 78)
        self.tree.setColumnWidth(3, 72)
        left_layout.addWidget(self.tree, 1)

        action_row = QHBoxLayout()
        action_row.setSpacing(8)
        self.btn_edit = QPushButton("✏ " + self._text("编辑", "編集"))
        set_button_role(self.btn_edit, "subtle")
        self.btn_edit.clicked.connect(self._edit)
        self.btn_delete = QPushButton("🗑 " + self._text("删除", "削除"))
        set_button_role(self.btn_delete, "danger")
        self.btn_delete.clicked.connect(self._delete)
        action_row.addWidget(self.btn_edit)
        action_row.addWidget(self.btn_delete)
        action_row.addStretch()
        left_layout.addLayout(action_row)
        splitter.addWidget(left)

        self.detail_panel = QFrame()
        self.detail_panel.setObjectName("ActionPanel")
        detail_layout = QVBoxLayout(self.detail_panel)
        detail_layout.setContentsMargins(14, 12, 14, 12)
        detail_layout.setSpacing(10)
        detail_header = QHBoxLayout()
        detail_header.setContentsMargins(0, 0, 0, 0)
        title_stack = QVBoxLayout()
        title_stack.setContentsMargins(0, 0, 0, 0)
        title_stack.setSpacing(2)
        self.lbl_title = QLabel(self._text("选择一首歌", "曲を選択"))
        self.lbl_title.setObjectName("SectionTitle")
        self.lbl_title.setWordWrap(True)
        self.lbl_meta = QLabel("")
        self.lbl_meta.setObjectName("MutedLabel")
        self.lbl_meta.setWordWrap(True)
        title_stack.addWidget(self.lbl_title)
        title_stack.addWidget(self.lbl_meta)
        detail_header.addLayout(title_stack, 1)
        self.btn_close_player = QPushButton("×")
        self.btn_close_player.setFixedWidth(34)
        set_button_role(self.btn_close_player, "subtle")
        self.btn_close_player.clicked.connect(self._hide_player_panel)
        detail_header.addWidget(self.btn_close_player)
        self.btn_expand_player = QPushButton("↗")
        self.btn_expand_player.setFixedWidth(34)
        self.btn_expand_player.setToolTip(self._text("展开歌词播放器", "歌詞プレーヤーを展開"))
        set_button_role(self.btn_expand_player, "subtle")
        self.btn_expand_player.clicked.connect(self._show_expanded_player)
        detail_header.addWidget(self.btn_expand_player)
        detail_layout.addLayout(detail_header)

        player_row = QHBoxLayout()
        player_row.setSpacing(8)
        self.btn_play = QPushButton("▶")
        self.btn_play.setFixedWidth(48)
        set_button_role(self.btn_play, "primary")
        self.btn_play.clicked.connect(self._toggle_play)
        self.btn_play.setEnabled(False)
        player_row.addWidget(self.btn_play)
        self.lbl_audio = QLabel("")
        self.lbl_audio.setObjectName("MutedLabel")
        self.lbl_audio.setWordWrap(True)
        player_row.addWidget(self.lbl_audio, 1)
        player_row.addWidget(QLabel(ui_text("playback_speed", self.lang)))
        self.speed_control = PlaybackSpeedControl(self._playback_rate, self)
        self.speed_control.rateChanged.connect(self._set_playback_rate)
        player_row.addWidget(self.speed_control)
        player_row.addWidget(QLabel(self._text("音调", "キー")))
        self.pitch_spin = self._create_pitch_combo()
        player_row.addWidget(self.pitch_spin)
        detail_layout.addLayout(player_row)

        progress_row = QHBoxLayout()
        progress_row.setSpacing(8)
        self.lbl_pos = QLabel("00:00")
        self.lbl_pos.setObjectName("MutedLabel")
        self.progress = ClickableSlider(Qt.Orientation.Horizontal)
        self.progress.setRange(0, 0)
        self.progress.sliderPressed.connect(lambda: setattr(self, "_slider_dragging", True))
        self.progress.sliderReleased.connect(self._seek_from_slider)
        self.progress.clickedValue.connect(self._seek_to_position)
        self.lbl_duration = QLabel("00:00")
        self.lbl_duration.setObjectName("MutedLabel")
        progress_row.addWidget(self.lbl_pos)
        progress_row.addWidget(self.progress, 1)
        progress_row.addWidget(self.lbl_duration)
        detail_layout.addLayout(progress_row)

        self.lyrics_view = QListWidget()
        self.lyrics_view.setObjectName("LyricsList")
        self.lyrics_view.setUniformItemSizes(False)
        self.lyrics_view.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.lyrics_view.setAlternatingRowColors(False)
        self.lyrics_view.itemPressed.connect(self._seek_to_lyric)
        self.lyrics_view.itemClicked.connect(self._seek_to_lyric)
        self.lyrics_view.itemActivated.connect(self._seek_to_lyric)
        self.lyrics_view.setContextMenuPolicy(Qt.ContextMenuPolicy.CustomContextMenu)
        self.lyrics_view.customContextMenuRequested.connect(self._on_lyrics_context)
        detail_layout.addWidget(self.lyrics_view, 1)
        splitter.addWidget(self.detail_panel)

        self.insight_panel = WordInsightPanel(self.lang)
        self.insight_panel.btn_close.clicked.connect(self._hide_insight_panel)
        self.insight_panel.addToAnkiRequested.connect(self._add_insight_to_anki)
        self.insight_panel.searchCorpusRequested.connect(self._search_insight_in_corpus)
        self.insight_panel.refreshDecksRequested.connect(lambda: self._refresh_insight_decks(self.insight_panel))
        self.insight_panel.hide()
        splitter.addWidget(self.insight_panel)
        self._splitter = splitter
        self.detail_panel.hide()
        splitter.setSizes([1280, 0, 0])

        self._build_expanded_player()

    def _build_expanded_player(self):
        self.expanded_player = QFrame(self)
        self.expanded_player.setObjectName("ExpandedPlayer")
        self.expanded_player.hide()
        self.expanded_player.setWindowFlags(Qt.WindowType.Widget)
        layout = QVBoxLayout(self.expanded_player)
        layout.setContentsMargins(34, 28, 34, 28)
        layout.setSpacing(18)

        header = QHBoxLayout()
        title_box = QVBoxLayout()
        title_box.setSpacing(4)
        self.lbl_expanded_title = QLabel(self._text("选择一首歌", "曲を選択"))
        self.lbl_expanded_title.setObjectName("ExpandedTitle")
        self.lbl_expanded_meta = QLabel("")
        self.lbl_expanded_meta.setObjectName("ExpandedMuted")
        title_box.addWidget(self.lbl_expanded_title)
        title_box.addWidget(self.lbl_expanded_meta)
        header.addLayout(title_box, 1)

        btn_display = QPushButton(self._text("显示", "表示"))
        btn_display.setToolTip(self._text("调整歌词字体、字号和行距", "歌詞フォント、サイズ、行間を調整"))
        set_button_role(btn_display, "subtle")
        btn_display.clicked.connect(self._open_display_settings)
        header.addWidget(btn_display)

        self.chk_furigana = QCheckBox(self._text("振假名", "ふりがな"))
        self.chk_furigana.setChecked(self._show_furigana)
        self.chk_furigana.toggled.connect(self._toggle_furigana)
        header.addWidget(self.chk_furigana)

        btn_collapse = QPushButton("↙")
        btn_collapse.setFixedWidth(38)
        btn_collapse.setToolTip(self._text("收起播放器", "プレーヤーを戻す"))
        set_button_role(btn_collapse, "subtle")
        btn_collapse.clicked.connect(self._hide_expanded_player)
        header.addWidget(btn_collapse)
        layout.addLayout(header)

        self.expanded_lyrics_view = LyricsPlayerView()
        self.expanded_lyrics_view.setFuriganaVisible(self._show_furigana)
        self.expanded_lyrics_view.setDisplaySettings(self._song_display)
        self.expanded_lyrics_view.lyricClicked.connect(self._seek_to_lyric_ms)
        self.expanded_lyrics_view.wordRequested.connect(lambda token: self._request_word_insight(token, expanded=True))

        self.expanded_content_splitter = QSplitter(Qt.Orientation.Horizontal)
        self.expanded_content_splitter.setChildrenCollapsible(False)
        self.expanded_content_splitter.addWidget(self.expanded_lyrics_view)
        self.expanded_insight_panel = WordInsightPanel(self.lang)
        self.expanded_insight_panel.btn_close.clicked.connect(lambda: self._hide_insight_panel(expanded=True))
        self.expanded_insight_panel.addToAnkiRequested.connect(self._add_insight_to_anki)
        self.expanded_insight_panel.searchCorpusRequested.connect(self._search_insight_in_corpus)
        self.expanded_insight_panel.refreshDecksRequested.connect(
            lambda: self._refresh_insight_decks(self.expanded_insight_panel)
        )
        self.expanded_insight_panel.hide()
        self.expanded_content_splitter.addWidget(self.expanded_insight_panel)
        self.expanded_content_splitter.setSizes([1200, 0])
        layout.addWidget(self.expanded_content_splitter, 1)

        controls = QFrame()
        controls.setObjectName("ExpandedControls")
        controls_layout = QVBoxLayout(controls)
        controls_layout.setContentsMargins(18, 14, 18, 14)
        controls_layout.setSpacing(12)

        progress_line = QHBoxLayout()
        progress_line.setContentsMargins(0, 0, 0, 0)
        progress_line.setSpacing(12)
        self.lbl_expanded_pos = QLabel("00:00")
        self.lbl_expanded_pos.setObjectName("PlayerTimeLabel")
        progress_line.addWidget(self.lbl_expanded_pos)
        self.expanded_progress = ClickableSlider(Qt.Orientation.Horizontal)
        self.expanded_progress.setObjectName("PlayerProgress")
        self.expanded_progress.setRange(0, 0)
        self.expanded_progress.sliderPressed.connect(lambda: setattr(self, "_slider_dragging", True))
        self.expanded_progress.sliderReleased.connect(self._seek_from_expanded_slider)
        self.expanded_progress.clickedValue.connect(self._seek_to_position)
        progress_line.addWidget(self.expanded_progress, 1)
        self.lbl_expanded_duration = QLabel("00:00")
        self.lbl_expanded_duration.setObjectName("PlayerTimeLabel")
        progress_line.addWidget(self.lbl_expanded_duration)
        controls_layout.addLayout(progress_line)

        action_line = QHBoxLayout()
        action_line.setContentsMargins(0, 0, 0, 0)
        action_line.setSpacing(10)
        self.btn_expanded_play = QPushButton("▶")
        self.btn_expanded_play.setFixedSize(48, 42)
        set_button_role(self.btn_expanded_play, "primary")
        self.btn_expanded_play.clicked.connect(self._toggle_play)
        action_line.addWidget(self.btn_expanded_play)
        action_line.addStretch(1)
        self.btn_playback_settings = QPushButton()
        self.btn_playback_settings.setObjectName("PlaybackSettingsButton")
        self.btn_playback_settings.setCheckable(True)
        self.btn_playback_settings.clicked.connect(self._toggle_playback_settings)
        action_line.addWidget(self.btn_playback_settings)
        controls_layout.addLayout(action_line)

        self.playback_settings_panel = QFrame()
        self.playback_settings_panel.setObjectName("PlaybackSettingsPanel")
        self.playback_settings_panel.setMaximumHeight(0)
        self.playback_settings_panel.hide()
        settings_layout = QHBoxLayout(self.playback_settings_panel)
        settings_layout.setContentsMargins(14, 12, 14, 12)
        settings_layout.setSpacing(10)
        speed_label = QLabel(ui_text("playback_speed", self.lang))
        speed_label.setObjectName("PlayerControlLabel")
        settings_layout.addWidget(speed_label)
        self.expanded_speed_control = PlaybackSpeedControl(self._playback_rate, self)
        self.expanded_speed_control.rateChanged.connect(self._set_playback_rate)
        settings_layout.addWidget(self.expanded_speed_control)
        settings_layout.addSpacing(10)
        pitch_label = QLabel(self._text("音调", "キー"))
        pitch_label.setObjectName("PlayerControlLabel")
        settings_layout.addWidget(pitch_label)
        self.expanded_pitch_spin = self._create_pitch_combo()
        settings_layout.addWidget(self.expanded_pitch_spin)
        settings_layout.addStretch(1)
        controls_layout.addWidget(self.playback_settings_panel)
        layout.addWidget(controls)
        self._position_expanded_player()
        self._apply_song_display_fonts()
        self._sync_playback_settings_button()

    def _position_expanded_player(self):
        if hasattr(self, "expanded_player"):
            self.expanded_player.setGeometry(self.rect())

    def resizeEvent(self, event):
        super().resizeEvent(event)
        self._position_expanded_player()

    def _apply_local_style(self, theme: str | None = None):
        theme = theme or self._theme
        self._theme = "light" if theme == "light" else "dark"
        if theme == "light":
            tree_bg = "#ffffff"
            tree_alt = "#f7f9fc"
            tree_sel = "#dbeafe"
            tree_sel_text = "#0f172a"
            tree_hover = "#eef4fb"
            artist_bg = "#eef3f8"
            artist_fg = "#243044"
            lyric_bg = "#ffffff"
            lyric_border = "#d9dee6"
            lyric_hover = "#eef4fb"
            lyric_sel = "#dbeafe"
            muted = "#64748b"
        else:
            tree_bg = "#1f2329"
            tree_alt = "#242930"
            tree_sel = "#273849"
            tree_sel_text = "#f4f7fb"
            tree_hover = "#252b33"
            artist_bg = "#232933"
            artist_fg = "#d8e2ef"
            lyric_bg = "#1f2329"
            lyric_border = "#333b46"
            lyric_hover = "#262d36"
            lyric_sel = "#26384b"
            muted = "#9aa6b5"
        self._artist_bg = artist_bg
        self._artist_fg = artist_fg
        self.setStyleSheet(f"""
            QTreeWidget {{
                background: {tree_bg};
                alternate-background-color: {tree_alt};
                font-size: 14px;
                outline: none;
                selection-background-color: {tree_sel};
                selection-color: {tree_sel_text};
            }}
            QTreeWidget::item {{
                min-height: 38px;
                padding: 3px 8px;
                background: transparent;
            }}
            QTreeWidget::item:alternate {{
                background: {tree_alt};
            }}
            QTreeWidget::item:hover {{
                background: {tree_hover};
            }}
            QTreeWidget::item:selected {{
                background: {tree_sel};
                color: {tree_sel_text};
            }}
            QTreeWidget::item:selected:active, QTreeWidget::item:selected:!active {{
                background: {tree_sel};
                color: {tree_sel_text};
            }}
            QTreeWidget::branch {{
                width: 22px;
            }}
            QTreeWidget::item[artistHeader="true"] {{
                background: {artist_bg};
                color: {artist_fg};
            }}
            QLabel#SectionTitle {{
                font-size: 16px;
                font-weight: 650;
            }}
            QLabel#MutedLabel {{
                color: {muted};
            }}
            QListWidget#LyricsList {{
                background: {lyric_bg};
                border: 1px solid {lyric_border};
                border-radius: 8px;
                padding: 8px;
                font-size: 15px;
                outline: none;
            }}
            QListWidget#LyricsList::item {{
                min-height: 34px;
                padding: 6px 8px;
                border-radius: 6px;
            }}
            QListWidget#LyricsList::item:hover {{
                background: {lyric_hover};
            }}
            QListWidget#LyricsList::item:selected {{
                background: {lyric_sel};
                color: {tree_sel_text};
            }}
            QSlider::groove:horizontal {{
                height: 5px;
                border-radius: 2px;
                background: {lyric_border};
            }}
            QSlider::sub-page:horizontal {{
                border-radius: 2px;
                background: #2f80ed;
            }}
            QSlider::handle:horizontal {{
                width: 14px;
                height: 14px;
                margin: -5px 0;
                border-radius: 7px;
                background: #ffffff;
                border: 2px solid #2f80ed;
            }}
            QSlider::handle:horizontal:hover {{
                background: #dbeafe;
            }}
            QFrame#ExpandedPlayer {{
                background: {tree_bg};
                border: 1px solid {lyric_border};
                border-radius: 12px;
            }}
            QLabel#ExpandedTitle {{
                color: {tree_sel_text};
                font-size: 24px;
                font-weight: 750;
            }}
            QLabel#ExpandedMuted {{
                color: {muted};
                font-size: 13px;
            }}
            QFrame#ExpandedControls {{
                background: {artist_bg};
                border: 1px solid {lyric_border};
                border-radius: 12px;
            }}
            QLabel#PlayerTimeLabel, QLabel#PlayerControlLabel {{
                color: {muted};
                font-size: 13px;
            }}
            QSlider#PlayerProgress::groove:horizontal {{
                height: 7px;
                border-radius: 3px;
                background: {lyric_border};
            }}
            QSlider#PlayerProgress::sub-page:horizontal {{
                border-radius: 3px;
                background: #2f80ed;
            }}
            QSlider#PlayerProgress::handle:horizontal {{
                width: 18px;
                height: 18px;
                margin: -6px 0;
                border-radius: 9px;
                background: #f8fbff;
                border: 3px solid #2f80ed;
            }}
            QPushButton#PlaybackSettingsButton {{
                min-height: 36px;
                border-radius: 18px;
                padding: 4px 16px;
                background: {lyric_bg};
                border: 1px solid {lyric_border};
                color: {tree_sel_text};
                font-weight: 650;
            }}
            QPushButton#PlaybackSettingsButton:hover,
            QPushButton#PlaybackSettingsButton:checked {{
                border-color: #2f80ed;
                background: {lyric_sel};
            }}
            QFrame#PlaybackSettingsPanel {{
                background: {lyric_bg};
                border: 1px solid {lyric_border};
                border-radius: 10px;
            }}
            QComboBox#PitchCombo {{
                min-height: 34px;
                border-radius: 17px;
                padding: 3px 12px;
                background: {artist_bg};
                border: 1px solid {lyric_border};
                color: {tree_sel_text};
            }}
            QComboBox#PitchCombo:hover {{
                border-color: #2f80ed;
            }}
            QFrame#WordInsightPanel {{
                background: {artist_bg};
                border: 1px solid {lyric_border};
                border-radius: 12px;
            }}
            QLabel#InsightWord {{
                color: {tree_sel_text};
                font-size: 22px;
                font-weight: 760;
            }}
            QLabel#InsightMeta, QLabel#InsightStats {{
                color: {muted};
                font-size: 13px;
            }}
            QTextBrowser#InsightBrowser {{
                background: {lyric_bg};
                border: 1px solid {lyric_border};
                border-radius: 10px;
                padding: 8px;
                color: {tree_sel_text};
            }}
            QPushButton#InsightSegment {{
                min-height: 32px;
                border-radius: 16px;
                padding: 4px 14px;
                background: {lyric_bg};
                border: 1px solid {lyric_border};
                color: {muted};
                font-weight: 650;
            }}
            QPushButton#InsightSegment:checked {{
                background: #2f80ed;
                border-color: #2f80ed;
                color: white;
            }}
            QComboBox#InsightDeckCombo {{
                min-height: 34px;
                border-radius: 17px;
                padding: 3px 12px;
                background: {lyric_bg};
                border: 1px solid {lyric_border};
                color: {tree_sel_text};
                selection-background-color: #2f80ed;
            }}
            QComboBox#InsightDeckCombo:hover, QComboBox#InsightDeckCombo:focus {{
                border-color: #2f80ed;
            }}
            QComboBox#InsightDeckCombo::drop-down {{
                width: 28px;
                border: none;
            }}
            QPushButton#InsightRefreshDecks {{
                min-height: 34px;
                border-radius: 17px;
                padding: 3px 12px;
            }}
            QListWidget#InsightExamples {{
                background: {lyric_bg};
                border: 1px solid {lyric_border};
                border-radius: 10px;
                padding: 8px;
                color: {tree_sel_text};
                outline: none;
            }}
            QListWidget#InsightExamples::item {{
                min-height: 58px;
                padding: 8px 10px;
                border-radius: 8px;
                margin: 2px 0;
            }}
            QListWidget#InsightExamples::item:hover {{
                background: {lyric_hover};
            }}
            QListWidget#InsightExamples::item:selected {{
                background: {lyric_sel};
            }}
            QListWidget#ExpandedLyrics {{
                background: {lyric_bg};
                border: 1px solid {lyric_border};
                border-radius: 12px;
                padding: 18px;
                outline: none;
            }}
            QListWidget#ExpandedLyrics::item {{
                min-height: 54px;
                padding: 8px 12px;
                border-radius: 10px;
            }}
            QListWidget#ExpandedLyrics::item:hover {{
                background: {lyric_hover};
            }}
            QListWidget#ExpandedLyrics::item:selected {{
                background: {lyric_sel};
                color: {tree_sel_text};
            }}
        """)

    def refresh_theme(self, theme: str):
        self._apply_local_style(theme)
        if hasattr(self, "expanded_lyrics_view"):
            self.expanded_lyrics_view.setTheme(self._theme)
        for panel_name in ("insight_panel", "expanded_insight_panel"):
            panel = getattr(self, panel_name, None)
            if panel is not None:
                panel.setTheme(self._theme)
        if hasattr(self, "tree"):
            for index in range(self.tree.topLevelItemCount()):
                item = self.tree.topLevelItem(index)
                if item.data(1, Qt.ItemDataRole.UserRole) == "artist":
                    for col in range(self.tree.columnCount()):
                        item.setBackground(col, QBrush(QColor(self._artist_bg)))
                        item.setForeground(col, QBrush(QColor(self._artist_fg)))
        for widget in self.findChildren(LyricLineWidget):
            widget.setActive(widget._active)
        self.update()

    def _load(self):
        conn = sqlite3.connect(str(DB_PATH))
        songs = conn.execute("""
            SELECT s.id, s.artist, s.title, s.year, s.album, s.genre, s.audio_path, COUNT(u.id)
            FROM songs s LEFT JOIN utterances u ON u.song_id = s.id
            GROUP BY s.id ORDER BY CAST(s.id AS INT)
        """).fetchall()
        conn.close()
        self._songs = [
            {
                "id": sid, "artist": artist, "title": title, "year": year or "",
                "album": album or "", "genre": genre or "", "audio_path": audio_path or "",
                "lines": lines,
            }
            for sid, artist, title, year, album, genre, audio_path, lines in songs
        ]
        self._song_by_id = {s["id"]: s for s in self._songs}
        self._rebuild_tree()

    def _matches_search(self, song: dict, keyword: str) -> bool:
        if not keyword:
            return True
        haystack = " ".join(str(song.get(k, "")) for k in ("id", "artist", "title", "year", "album", "genre")).lower()
        return keyword in haystack

    def _rebuild_tree(self):
        keyword = self.search_edit.text().strip().lower()
        rows = [s for s in self._songs if self._matches_search(s, keyword)]
        self.tree.clear()
        grouped = self.chk_group.isChecked()
        if grouped:
            artist_map: dict[str, list[dict]] = {}
            for song in rows:
                artist_map.setdefault(song["artist"], []).append(song)
            for artist, songs in sorted(artist_map.items(), key=lambda item: (-len(item[1]), item[0])):
                total_lines = sum(s["lines"] for s in songs)
                root = QTreeWidgetItem(["", artist, "", str(total_lines), f"{len(songs)}"])
                root.setFirstColumnSpanned(False)
                root.setData(0, Qt.ItemDataRole.UserRole, "")
                root.setData(1, Qt.ItemDataRole.UserRole, "artist")
                root.setToolTip(1, self._text(f"{artist}，{len(songs)} 首歌", f"{artist}、{len(songs)} 曲"))
                font = root.font(1)
                font.setBold(True)
                for col in range(self.tree.columnCount()):
                    root.setFont(col, font)
                    root.setBackground(col, QBrush(QColor(self._artist_bg)))
                    root.setForeground(col, QBrush(QColor(self._artist_fg)))
                root.setText(4, f"{len(songs)} " + self._text("首", "曲"))
                root.setExpanded(bool(keyword) or len(artist_map) <= 8)
                self.tree.addTopLevelItem(root)
                for song in songs:
                    self._add_song_item(root, song, child=True)
        else:
            for song in rows:
                self._add_song_item(self.tree, song, child=False)
        self.lbl_count.setText(self._text(f"{len(rows)} 首", f"{len(rows)} 曲"))
        if self._current_song_id and self._current_song_id in self._song_by_id:
            self._select_song(self._current_song_id)

    def _add_song_item(self, parent, song: dict, *, child: bool):
        values = [
            song["id"],
            ("    " + song["title"]) if child else f'{song["artist"]}  -  {song["title"]}',
            song["year"],
            str(song["lines"]),
            "▶" if pathlib.Path(song["audio_path"]).exists() else "",
        ]
        item = QTreeWidgetItem(values)
        item.setData(0, Qt.ItemDataRole.UserRole, song["id"])
        item.setToolTip(1, f'{song["artist"]} - {song["title"]}')
        if isinstance(parent, QTreeWidget):
            parent.addTopLevelItem(item)
        else:
            parent.addChild(item)

    def _select_song(self, song_id: str):
        matches = self.tree.findItems(song_id, Qt.MatchFlag.MatchExactly | Qt.MatchFlag.MatchRecursive, 0)
        if matches:
            self.tree.setCurrentItem(matches[0])

    def _selected_song(self):
        item = self.tree.currentItem()
        if not item:
            QMessageBox.information(self, "", self._text("请选择歌曲", "曲を選択してください"))
            return None
        sid = item.data(0, Qt.ItemDataRole.UserRole)
        if not sid:
            QMessageBox.information(self, "", self._text("请选择具体歌曲", "曲を選択してください"))
            return None
        return self._song_by_id.get(sid)

    def _on_selection_changed(self):
        song = self._selected_song_silent()
        if not song:
            return
        self._show_player_panel()
        self._current_song_id = song["id"]
        self.lbl_title.setText(f'{song["artist"]} - {song["title"]}')
        self.lbl_expanded_title.setText(f'{song["artist"]} - {song["title"]}')
        meta = []
        if song["year"]:
            meta.append(song["year"])
        if song["album"]:
            meta.append(song["album"])
        if song["genre"]:
            meta.append(song["genre"])
        meta.append(self._text(f'{song["lines"]} 行歌词', f'{song["lines"]} 行'))
        self.lbl_meta.setText(" / ".join(meta))
        self.lbl_expanded_meta.setText(" / ".join(meta))
        audio_path = pathlib.Path(song["audio_path"])
        self.lbl_audio.setText(audio_path.name if audio_path.exists() else self._text("未找到音频文件", "音声ファイルなし"))
        self._update_pitch_status()
        self.btn_play.setEnabled(audio_path.exists())
        self.btn_expanded_play.setEnabled(audio_path.exists())
        self.progress.setEnabled(audio_path.exists())
        self.expanded_progress.setEnabled(audio_path.exists())
        if str(audio_path) != self._current_original_audio_path:
            self._player.stop()
            self._current_original_audio_path = str(audio_path) if audio_path.exists() else ""
            self._current_audio_path = ""
            if audio_path.exists():
                self._set_player_source(str(audio_path), 0)
            self.progress.setValue(0)
            self.expanded_progress.setValue(0)
            self.lbl_pos.setText("00:00")
            self.lbl_expanded_pos.setText("00:00")
            self.lbl_duration.setText("00:00")
            self.lbl_expanded_duration.setText("00:00")
            self._duration_ms = 0
        self._load_lyrics(song["id"])

    def _show_player_panel(self):
        if not self.detail_panel.isVisible():
            self.detail_panel.show()
            fade_in_widget(self.detail_panel, 120)
        self._animate_panel(opening=True)

    def _hide_player_panel(self):
        if self._player.playbackState() == QMediaPlayer.PlaybackState.PlayingState:
            self._player.pause()
        self.insight_panel.hide()
        self.tree.clearSelection()
        self._animate_panel(opening=False)

    def _animate_panel(self, *, opening: bool):
        total = max(self._splitter.width(), 1)
        target = min(520, max(420, int(total * 0.36))) if opening else 0
        insight = 360 if opening and self.insight_panel.isVisible() else 0
        current = self.detail_panel.width() if self.detail_panel.isVisible() else 0
        if opening and current <= 8:
            current = 0
            self._splitter.setSizes([total, 0, insight])

        anim = QPropertyAnimation(self.detail_panel, b"maximumWidth", self)
        anim.setDuration(180)
        anim.setStartValue(current)
        anim.setEndValue(target)
        anim.setEasingCurve(QEasingCurve.Type.OutCubic)

        def _tick(value):
            right = int(value)
            self._splitter.setSizes([max(total - right - insight, 280), right, insight])

        def _finish():
            if opening:
                self.detail_panel.setMaximumWidth(16777215)
                self._splitter.setSizes([max(total - target - insight, 280), target, insight])
            else:
                self.detail_panel.hide()
                self.detail_panel.setMaximumWidth(16777215)
                self._splitter.setSizes([total, 0, 0])

        anim.valueChanged.connect(_tick)
        anim.finished.connect(_finish)
        self._panel_anim = anim
        anim.start()

    def _selected_song_silent(self):
        item = self.tree.currentItem()
        if not item:
            return None
        sid = item.data(0, Qt.ItemDataRole.UserRole)
        return self._song_by_id.get(sid) if sid else None

    def _load_lyrics(self, song_id: str):
        try:
            conn = sqlite3.connect(str(DB_PATH))
            rows = conn.execute(
                "SELECT time_sec, text FROM utterances WHERE song_id=? ORDER BY line_idx, time_sec",
                (song_id,)
            ).fetchall()
            conn.close()
        except Exception as e:
            self.lyrics_view.clear()
            self.expanded_lyrics_view.setLyrics([])
            self.lyrics_view.addItem(str(e))
            return
        self._lyric_data = []
        for time_sec, text in rows:
            mins = int(time_sec // 60)
            secs = time_sec % 60
            self._lyric_data.append((int(time_sec * 1000), f"{mins:02d}:{secs:05.2f}", text))
        self._render_lyrics()

    def _render_lyrics(self):
        self._render_lyrics_view(
            self.lyrics_view, self._lyric_rows, expanded=False, current_item_attr="_current_lyric_item"
        )
        self._expanded_lyric_rows.clear()
        self._current_expanded_lyric_item = None
        self.expanded_lyrics_view.setLyrics(self._lyric_data)
        self._highlight_current_lyric(self._player.position(), force=True)

    def _render_lyrics_view(self, view: QListWidget, store: list, *, expanded: bool, current_item_attr: str):
        view.clear()
        store.clear()
        setattr(self, current_item_attr, None)
        for ms, time_text, lyric in self._lyric_data:
            item = QListWidgetItem()
            item.setData(Qt.ItemDataRole.UserRole, ms)
            widget = LyricLineWidget(
                time_text, lyric, furigana=self._show_furigana,
                active=False, expanded=expanded, display=self._song_display
            )
            item.setSizeHint(widget.sizeHint())
            view.addItem(item)
            view.setItemWidget(item, widget)
            store.append((ms, item))

    def _on_lyrics_context(self, pos):
        item = self.lyrics_view.itemAt(pos)
        if not item:
            return
        row = self.lyrics_view.row(item)
        if not (0 <= row < len(self._lyric_data)):
            return
        ms, time_text, lyric = self._lyric_data[row]
        line_widget = self.lyrics_view.itemWidget(item)
        if not isinstance(line_widget, LyricLineWidget):
            return
        local_pos = line_widget.mapFrom(self.lyrics_view.viewport(), pos)
        token = line_widget.tokenAtPosition(local_pos)
        if not token:
            return
        token.update({"lyric": lyric, "time_ms": ms, "time_text": time_text})
        self._request_word_insight(token, expanded=False)

    def _request_word_insight(self, token: dict, *, expanded: bool = False):
        if not token:
            return
        panel = self.expanded_insight_panel if expanded and self._expanded_player_open else self.insight_panel
        self._active_insight_panel = panel
        self._insight_request_id += 1
        request_id = self._insight_request_id
        was_visible = panel.isVisible()
        panel.set_loading(token)
        dict_key = _insight_dict_key(token)
        corpus_key = _insight_corpus_key(token, self._current_song_id)
        full_key = (dict_key, corpus_key)
        cached_full = _insight_cache_get(_WORD_FULL_CACHE, full_key)
        needs_worker = cached_full is None
        if cached_full is not None:
            panel.set_data(cached_full)
        else:
            cached_dict = _insight_cache_get(_WORD_DICT_CACHE, dict_key)
            if cached_dict is not None:
                panel.set_data(cached_dict, update_examples=False)
            cached_corpus = _insight_cache_get(_WORD_CORPUS_CACHE, corpus_key)
            if cached_corpus is not None:
                merged = dict(cached_dict or _base_insight_data(token))
                merged.update(cached_corpus)
                panel.set_data(merged)
        if panel is self.expanded_insight_panel:
            panel.show()
            total = max(900, self.expanded_content_splitter.width())
            self.expanded_content_splitter.setSizes([max(520, total - 390), 390])
        else:
            panel.show()
            self.detail_panel.show()
            total = max(900, self._splitter.width())
            self._splitter.setSizes([max(420, total - 790), 430, 360])
        if not was_visible:
            fade_in_widget(panel, 140)
        if not self._deck_list_requested:
            self._refresh_insight_decks(panel)

        if not needs_worker:
            return
        worker = WordInsightWorker(token, self._current_song_id, self)

        def _partial(data, req=request_id, target=panel):
            if req != self._insight_request_id or target is not self._active_insight_panel:
                return
            target.set_data(data, update_examples=False)

        def _done(data, req=request_id, target=panel):
            if req != self._insight_request_id or target is not self._active_insight_panel:
                return
            target.set_data(data, update_examples=True)

        worker.partialReady.connect(_partial)
        worker.ready.connect(_done)
        def _cleanup_worker():
            try:
                self._insight_workers.remove(worker)
            except ValueError:
                pass
            if self._insight_worker is worker:
                self._insight_worker = None

        worker.finished.connect(_cleanup_worker)
        worker.finished.connect(worker.deleteLater)
        self._insight_worker = worker
        self._insight_workers.append(worker)
        worker.start()

    def _refresh_insight_decks(self, target_panel=None):
        try:
            if self._deck_worker and self._deck_worker.isRunning():
                return
        except RuntimeError:
            self._deck_worker = None
        self._deck_list_requested = True
        panels = [
            panel for panel in (
                getattr(self, "insight_panel", None),
                getattr(self, "expanded_insight_panel", None),
            )
            if isinstance(panel, WordInsightPanel)
        ]
        for panel in panels:
            panel.btn_refresh_decks.setEnabled(False)
            panel.btn_refresh_decks.setText(self._text("刷新中", "更新中"))

        worker = DeckListWorker(self)

        def _done(decks, err):
            for panel in panels:
                panel.btn_refresh_decks.setEnabled(True)
                panel.btn_refresh_decks.setText(self._text("刷新", "更新"))
                if err:
                    panel.btn_refresh_decks.setToolTip(err)
                else:
                    panel.btn_refresh_decks.setToolTip("")
                    panel.set_decks(decks, panel.selected_deck())

        worker.ready.connect(_done)
        def _clear_worker():
            if self._deck_worker is worker:
                self._deck_worker = None

        worker.finished.connect(_clear_worker)
        worker.finished.connect(worker.deleteLater)
        self._deck_worker = worker
        worker.start()

    def _hide_insight_panel(self, expanded: bool = False):
        panel = self.expanded_insight_panel if expanded else self.insight_panel
        if not panel.isVisible():
            return
        panel.hide()
        if panel is self.expanded_insight_panel:
            self.expanded_content_splitter.setSizes([1, 0])
        else:
            left = max(600, self._splitter.width() - 430)
            self._splitter.setSizes([left, 430 if self.detail_panel.isVisible() else 0, 0])

    def _add_insight_to_anki(self, data: dict):
        lemma = data.get("lemma") or data.get("surface") or ""
        if not lemma:
            return
        selected = data.get("selected_examples") or []
        if not selected:
            answer = QMessageBox.question(
                self,
                "Anki",
                self._text(
                    "没有勾选例句，将只发送词条和释义。是否继续？",
                    "例文が選択されていません。語義のみ送信しますか？",
                ),
                QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
                QMessageBox.StandardButton.No,
            )
            if answer != QMessageBox.StandardButton.Yes:
                return
        deck = (data.get("deck") or "JPOP Corpus").strip() or "JPOP Corpus"
        data = dict(data)
        data["deck"] = deck
        cfg = load_settings()
        anki_cfg = dict(cfg.get("anki_dialog", {}))
        anki_cfg["deck"] = deck
        cfg["anki_dialog"] = anki_cfg
        save_settings(cfg)
        self._set_insight_busy(True)
        worker = QuickAnkiAddWorker(data, self)
        worker.finished.connect(self._on_quick_anki_done)
        worker.finished.connect(worker.deleteLater)
        self._anki_quick_worker = worker
        worker.start()

    def _set_insight_busy(self, busy: bool):
        for panel in (getattr(self, "insight_panel", None), getattr(self, "expanded_insight_panel", None)):
            if panel:
                panel.btn_anki.setEnabled(not busy)
                panel.deck_combo.setEnabled(not busy)
                panel.btn_refresh_decks.setEnabled(not busy)
                if busy:
                    panel.btn_anki.setText(self._text("发送中…", "送信中…"))
                else:
                    panel.btn_anki.setText(self._text("加入 Anki", "Anki に追加"))

    def _on_quick_anki_done(self, ok: bool, message: str):
        self._set_insight_busy(False)
        if ok:
            QMessageBox.information(self, "Anki", message)
        else:
            QMessageBox.warning(self, "Anki", message)

    def _search_insight_in_corpus(self, lemma: str):
        if not lemma:
            return
        panel = self._active_insight_panel
        if isinstance(panel, WordInsightPanel):
            panel._set_view_mode("examples")

    def _apply_song_display_fonts(self):
        if not hasattr(self, "tree"):
            return
        base_size = int(self._song_display.get("base_size", 18))
        list_font = _lyric_font(self._song_display, max(10, base_size - 5))
        self.tree.setFont(list_font)
        self.lyrics_view.setFont(_lyric_font(self._song_display, max(11, base_size - 4)))
        if hasattr(self, "expanded_lyrics_view"):
            self.expanded_lyrics_view.setFont(_lyric_font(self._song_display, base_size))
            self.expanded_lyrics_view.setDisplaySettings(self._song_display)

    def _open_display_settings(self):
        dlg = SongDisplaySettingsDialog(self)
        if dlg.exec() != QDialog.DialogCode.Accepted:
            return
        cfg = load_settings()
        cfg.update(dlg.result_settings())
        save_settings(cfg)
        load_custom_fonts(cfg.get("custom_font_paths", []))
        self._song_display = _song_display_settings()
        self._apply_song_display_fonts()
        self._render_lyrics()

    def _toggle_furigana(self, checked: bool):
        self._show_furigana = bool(checked)
        cfg = load_settings()
        cfg["show_furigana"] = self._show_furigana
        save_settings(cfg)
        if hasattr(self, "chk_furigana"):
            self.chk_furigana.blockSignals(True)
            self.chk_furigana.setChecked(self._show_furigana)
            self.chk_furigana.blockSignals(False)
        if hasattr(self, "expanded_lyrics_view"):
            self.expanded_lyrics_view.setFuriganaVisible(self._show_furigana)
        self._render_lyrics()

    def _show_expanded_player(self):
        song = self._selected_song_silent()
        if not song:
            QMessageBox.information(self, "", self._text("请选择歌曲", "曲を選択してください"))
            return
        self._expanded_player_open = True
        self._position_expanded_player()
        self.expanded_player.raise_()
        self.expanded_player.show()
        self.expanded_player.setGraphicsEffect(None)
        self._sync_speed_buttons()
        self._sync_play_button(self._player.playbackState())
        self._on_duration_changed(self._duration_ms)
        self._on_position_changed(self._player.position())
        fade_in_widget(self.expanded_player, 180)

    def _hide_expanded_player(self):
        self._expanded_player_open = False
        if not getattr(self, "expanded_player", None) or not self.expanded_player.isVisible():
            return
        effect = QGraphicsOpacityEffect(self.expanded_player)
        effect.setOpacity(1.0)
        self.expanded_player.setGraphicsEffect(effect)
        anim = QPropertyAnimation(effect, b"opacity", self)
        anim.setDuration(150)
        anim.setStartValue(1.0)
        anim.setEndValue(0.0)
        anim.setEasingCurve(QEasingCurve.Type.OutCubic)

        def _finish():
            self.expanded_player.hide()
            self.expanded_player.setGraphicsEffect(None)

        anim.finished.connect(_finish)
        self._expanded_anim = anim
        anim.start()

    def _set_player_source(self, path: str, position: int | None = None):
        if not path:
            return
        if path != self._current_audio_path:
            self._current_audio_path = path
            self._player.setSource(QUrl.fromLocalFile(path))
        if position is not None:
            target = max(0, int(position))
            self._player.setPosition(target)
            QTimer.singleShot(80, lambda pos=target: self._player.setPosition(pos))

    def _pitch_label(self) -> str:
        if self._pitch_semitones == 0:
            return self._text("原调", "原キー")
        return f"{self._pitch_semitones:+d}" + self._text(" 半音", " 半音")

    def _playback_settings_summary(self) -> str:
        return f"⚙ {playback_rate_label(self._playback_rate)} / {self._pitch_label()}"

    def _sync_playback_settings_button(self):
        btn = getattr(self, "btn_playback_settings", None)
        if btn is not None:
            btn.setText(self._playback_settings_summary())

    def _toggle_playback_settings(self):
        panel = getattr(self, "playback_settings_panel", None)
        if panel is None:
            return
        opening = not self._playback_settings_open
        self._playback_settings_open = opening
        self.btn_playback_settings.setChecked(opening)
        if opening:
            panel.show()
        start = panel.maximumHeight()
        end = 62 if opening else 0
        anim = QPropertyAnimation(panel, b"maximumHeight", self)
        anim.setDuration(180)
        anim.setStartValue(start)
        anim.setEndValue(end)
        anim.setEasingCurve(QEasingCurve.Type.OutCubic)

        def _finish():
            if not opening:
                panel.hide()

        anim.finished.connect(_finish)
        self._playback_settings_anim = anim
        anim.start()

    def _pitch_option_label(self, semitones: int) -> str:
        if semitones == 0:
            return self._text("原调", "原キー")
        return f"{semitones:+d} " + self._text("半音", "半音")

    def _create_pitch_combo(self):
        combo = QComboBox()
        combo.setObjectName("PitchCombo")
        for value in range(-6, 7):
            combo.addItem(self._pitch_option_label(value), value)
        combo.setCurrentIndex(max(0, combo.findData(self._pitch_semitones)))
        combo.setMinimumWidth(112)
        combo.setToolTip(self._text("升调/降调，首次切换会生成缓存音频", "キー変更。初回はキャッシュ音声を生成します"))
        combo.currentIndexChanged.connect(lambda *_args, c=combo: self._set_pitch_shift(c.currentData()))
        return combo

    def _pitch_cache_path(self, source: str, semitones: int) -> pathlib.Path:
        src = pathlib.Path(source)
        try:
            stat = src.stat()
            sig = f"{src.resolve()}|{stat.st_mtime_ns}|{stat.st_size}|{semitones}"
        except Exception:
            sig = f"{src}|{semitones}"
        digest = hashlib.sha1(sig.encode("utf-8", "ignore")).hexdigest()[:16]
        safe_stem = re.sub(r"[^0-9A-Za-z._-]+", "_", src.stem).strip("._") or "song"
        return app_dir() / "output" / "pitch_cache" / f"{safe_stem}_{semitones:+d}_{digest}.flac"

    def _sync_pitch_controls(self):
        for spin in (getattr(self, "pitch_spin", None), getattr(self, "expanded_pitch_spin", None)):
            if spin is None:
                continue
            spin.blockSignals(True)
            if hasattr(spin, "findData"):
                spin.setCurrentIndex(max(0, spin.findData(self._pitch_semitones)))
            else:
                spin.setValue(self._pitch_semitones)
            spin.blockSignals(False)

    def _set_pitch_controls_enabled(self, enabled: bool):
        for spin in (getattr(self, "pitch_spin", None), getattr(self, "expanded_pitch_spin", None)):
            if spin is not None:
                spin.setEnabled(enabled)

    def _set_pitch_shift(self, semitones: int):
        self._pitch_semitones = _clamp_int(semitones, 0, -6, 6)
        cfg = load_settings()
        cfg["song_pitch_semitones"] = self._pitch_semitones
        save_settings(cfg)
        self._sync_pitch_controls()
        self._sync_playback_settings_button()
        song = self._selected_song_silent()
        if not song:
            return
        audio_path = pathlib.Path(song["audio_path"])
        if not audio_path.exists():
            return
        position = self._player.position()
        autoplay = self._player.playbackState() == QMediaPlayer.PlaybackState.PlayingState
        self._prepare_pitch_source(str(audio_path), position=position, autoplay=autoplay)

    def _prepare_pitch_source(self, source: str, *, position: int, autoplay: bool):
        if self._pitch_semitones == 0:
            self._pitch_processing = False
            self._set_player_source(source, position)
            self._player.setPlaybackRate(self._playback_rate)
            if autoplay:
                self._player.play()
            self._update_pitch_status()
            return

        cache_path = self._pitch_cache_path(source, self._pitch_semitones)
        if cache_path.exists() and cache_path.stat().st_size > 0:
            self._pitch_processing = False
            self._set_player_source(str(cache_path), position)
            self._player.setPlaybackRate(self._playback_rate)
            if autoplay:
                self._player.play()
            self._update_pitch_status()
            return

        if self._pitch_worker and self._pitch_worker.isRunning():
            self._pitch_worker.requestInterruption()
            self._pitch_worker.quit()
            self._pitch_worker.wait(500)
        self._pitch_processing = True
        self._update_pitch_status(processing=True)
        self.btn_play.setEnabled(False)
        self.btn_expanded_play.setEnabled(False)
        self._set_pitch_controls_enabled(False)
        worker = PitchShiftWorker(source, str(cache_path), self._pitch_semitones, position, autoplay, self)
        worker.finished.connect(self._on_pitch_ready)
        self._pitch_worker = worker
        worker.start()

    def _on_pitch_ready(self, source: str, semitones: int, output: str, position: int, autoplay: bool, error: str):
        self._pitch_processing = False
        song = self._selected_song_silent()
        audio_exists = bool(song and pathlib.Path(song["audio_path"]).exists())
        self.btn_play.setEnabled(audio_exists)
        self.btn_expanded_play.setEnabled(audio_exists)
        self._set_pitch_controls_enabled(True)
        if not song or str(pathlib.Path(song["audio_path"])) != source or semitones != self._pitch_semitones:
            self._update_pitch_status()
            return
        if error or not output:
            QMessageBox.warning(
                self,
                self._text("变调失败", "キー変更に失敗"),
                self._text("无法生成变调音频：", "キー変更音声を生成できません：") + (error or ""),
            )
            self._pitch_semitones = 0
            self._sync_pitch_controls()
            cfg = load_settings()
            cfg["song_pitch_semitones"] = 0
            save_settings(cfg)
            self._set_player_source(source, position)
            self._update_pitch_status()
            return
        self._set_player_source(output, position)
        self._player.setPlaybackRate(self._playback_rate)
        if autoplay:
            self._player.play()
        self._update_pitch_status()

    def _update_pitch_status(self, *, processing: bool = False):
        song = self._selected_song_silent()
        if not song:
            return
        audio_path = pathlib.Path(song["audio_path"])
        base = audio_path.name if audio_path.exists() else self._text("未找到音频文件", "音声ファイルなし")
        if processing:
            suffix = self._text(f" · 正在生成 {self._pitch_label()} 缓存…", f" · {self._pitch_label()} を生成中…")
        elif self._pitch_semitones != 0:
            suffix = f" · {self._pitch_label()}"
        else:
            suffix = ""
        self.lbl_audio.setText(base + suffix)

    def _toggle_play(self):
        song = self._selected_song_silent()
        if not song:
            return
        audio_path = pathlib.Path(song["audio_path"])
        if not audio_path.exists():
            QMessageBox.warning(self, self._text("音频缺失", "音声なし"), str(audio_path))
            return
        if self._player.playbackState() == QMediaPlayer.PlaybackState.PlayingState:
            self._player.pause()
            return
        self._current_original_audio_path = str(audio_path)
        self._prepare_pitch_source(
            str(audio_path),
            position=self._player.position() if self._current_audio_path else 0,
            autoplay=True,
        )

    def _sync_speed_buttons(self):
        rate = normalize_playback_rate(getattr(self, "_playback_rate", load_settings().get("playback_rate", 1.0)))
        if hasattr(self, "speed_control"):
            self.speed_control.blockSignals(True)
            self.speed_control.setRate(rate, emit=False)
            self.speed_control.blockSignals(False)
        if hasattr(self, "expanded_speed_control"):
            self.expanded_speed_control.blockSignals(True)
            self.expanded_speed_control.setRate(rate, emit=False)
            self.expanded_speed_control.blockSignals(False)

    def _set_playback_rate(self, rate: float):
        self._playback_rate = normalize_playback_rate(rate)
        self._player.setPlaybackRate(self._playback_rate)
        cfg = load_settings()
        cfg["playback_rate"] = self._playback_rate
        save_settings(cfg)
        self._sync_speed_buttons()
        self._sync_playback_settings_button()

    def refresh_playback_rate_from_settings(self):
        self._playback_rate = normalize_playback_rate(load_settings().get("playback_rate", self._playback_rate))
        self._player.setPlaybackRate(self._playback_rate)
        self._sync_speed_buttons()

    def _sync_play_button(self, state):
        playing = state == QMediaPlayer.PlaybackState.PlayingState
        self.btn_play.setText("⏸" if playing else "▶")
        if hasattr(self, "btn_expanded_play"):
            self.btn_expanded_play.setText("⏸" if playing else "▶")

    def _on_duration_changed(self, duration: int):
        self._duration_ms = max(duration, 0)
        self.progress.setRange(0, self._duration_ms)
        self.lbl_duration.setText(self._format_ms(self._duration_ms))
        if hasattr(self, "expanded_progress"):
            self.expanded_progress.setRange(0, self._duration_ms)
            self.lbl_expanded_duration.setText(self._format_ms(self._duration_ms))

    def _on_position_changed(self, position: int):
        if not self._slider_dragging:
            self.progress.setValue(position)
            if hasattr(self, "expanded_progress"):
                self.expanded_progress.setValue(position)
        self.lbl_pos.setText(self._format_ms(position))
        if hasattr(self, "lbl_expanded_pos"):
            self.lbl_expanded_pos.setText(self._format_ms(position))
        self._highlight_current_lyric(position)

    def _seek_from_slider(self):
        self._slider_dragging = False
        self._seek_to_position(self.progress.value())

    def _seek_from_expanded_slider(self):
        self._slider_dragging = False
        self._seek_to_position(self.expanded_progress.value())

    def _seek_to_position(self, value: int):
        target = max(0, min(int(value), self._duration_ms if self._duration_ms else int(value)))
        self._player.setPosition(target)
        self.progress.setValue(target)
        if hasattr(self, "expanded_progress"):
            self.expanded_progress.setValue(target)
        self.lbl_pos.setText(self._format_ms(target))
        if hasattr(self, "lbl_expanded_pos"):
            self.lbl_expanded_pos.setText(self._format_ms(target))
        self._highlight_current_lyric(target)

    def _seek_to_lyric(self, item: QListWidgetItem):
        ms = item.data(Qt.ItemDataRole.UserRole)
        if ms is None:
            return
        self._seek_to_lyric_ms(int(ms))

    def _seek_to_lyric_ms(self, target: int):
        song = self._selected_song_silent()
        if not song:
            return
        audio_path = pathlib.Path(song["audio_path"])
        if not audio_path.exists():
            return
        target = int(target)
        self._current_original_audio_path = str(audio_path)
        self._prepare_pitch_source(str(audio_path), position=target, autoplay=True)

    def _highlight_current_lyric(self, position: int, *, force: bool = False):
        self._highlight_lyric_view(
            self.lyrics_view, self._lyric_rows, "_current_lyric_item",
            position, force=force
        )
        if hasattr(self, "expanded_lyrics_view"):
            self.expanded_lyrics_view.setPosition(position, force=force)

    def _highlight_lyric_view(self, view: QListWidget, rows: list, current_attr: str,
                              position: int, *, force: bool = False):
        if not rows:
            return
        current_item = rows[0][1]
        for ms, item in rows:
            if ms <= position:
                current_item = item
            else:
                break
        previous_item = getattr(self, current_attr, None)
        if force or previous_item is not current_item:
            if previous_item:
                previous_widget = view.itemWidget(previous_item)
                if hasattr(previous_widget, "setActive"):
                    previous_widget.setActive(False)
            current_widget = view.itemWidget(current_item)
            if hasattr(current_widget, "setActive"):
                current_widget.setActive(True)
            setattr(self, current_attr, current_item)
            view.setCurrentItem(current_item)
            if view.isVisible():
                view.scrollToItem(current_item, QListWidget.ScrollHint.PositionAtCenter)

    def _format_ms(self, ms: int) -> str:
        total = max(0, int(ms // 1000))
        mins = total // 60
        secs = total % 60
        return f"{mins:02d}:{secs:02d}"

    def _edit(self):
        s = self._selected_song()
        if s is None:
            return
        conn = sqlite3.connect(str(DB_PATH))
        row = conn.execute(
            "SELECT id,title,artist,year,album,genre,audio_path FROM songs WHERE id=?", (s["id"],)
        ).fetchone()
        conn.close()

        dlg = SongEditDialog(row, self)
        if dlg.exec() != QDialog.DialogCode.Accepted:
            return
        data = dlg.get_data()
        conn = sqlite3.connect(str(DB_PATH))
        conn.execute(
            "UPDATE songs SET title=?,artist=?,year=?,album=?,genre=?,audio_path=? WHERE id=?",
            (data["title"], data["artist"], data["year"],
             data["album"], data["genre"], data["audio_path"], s["id"])
        )
        conn.commit()
        conn.close()
        self._update_csv(s["id"], data)
        self._load()
        self._select_song(s["id"])

    def _delete(self):
        s = self._selected_song()
        if s is None:
            return
        sid, artist, title = s["id"], s["artist"], s["title"]
        if QMessageBox.question(
            self, self._text("删除歌曲", "曲を削除"),
            self._text(
                f"{artist}《{title}》会被删除，歌词数据也会一并删除。\n继续吗？",
                f"{artist}《{title}》を削除しますか？\n歌詞データもすべて削除されます。"
            )
        ) != QMessageBox.StandardButton.Yes:
            return

        conn = sqlite3.connect(str(DB_PATH))
        utt_ids = [r[0] for r in conn.execute(
            "SELECT id FROM utterances WHERE song_id=?", (sid,)
        ).fetchall()]
        if utt_ids:
            ph = ",".join("?" * len(utt_ids))
            conn.execute(f"DELETE FROM utterances_fts WHERE rowid IN ({ph})", utt_ids)
            conn.execute(f"DELETE FROM tokens WHERE utterance_id IN ({ph})", utt_ids)
        conn.execute("DELETE FROM utterances WHERE song_id=?", (sid,))
        conn.execute("DELETE FROM songs WHERE id=?", (sid,))
        conn.commit()
        conn.close()

        with open(CSV_PATH, encoding="utf-8-sig", newline="") as f:
            rows = [r for r in csv.DictReader(f) if r["id"] != sid]
        with open(CSV_PATH, "w", encoding="utf-8-sig", newline="") as f:
            w = csv.DictWriter(f, fieldnames=["id","title","artist","year","album","genre","audio_path"])
            w.writeheader(); w.writerows(rows)
        self._load()

    def _update_csv(self, sid: str, data: dict):
        with open(CSV_PATH, encoding="utf-8-sig", newline="") as f:
            rows = list(csv.DictReader(f))
        for r in rows:
            if r["id"] == sid:
                r.update(data)
        with open(CSV_PATH, "w", encoding="utf-8-sig", newline="") as f:
            w = csv.DictWriter(f, fieldnames=["id","title","artist","year","album","genre","audio_path"])
            w.writeheader(); w.writerows(rows)


# ------------------------------------------------------------------ MultiSelectComboBox
