"""
gui.py — JPOP 語料庫 KWIC 検索ツール
双击结果行从对应时间戳播放音频。
"""

import sys
import csv
import html
import json
import re
import shutil
import sqlite3
import pathlib

from anki_connect import request as _anki_request
from anki_learning import load_learning_status
from project_paths import AUDIO_DIR, CSV_PATH, DB_PATH, LRC_DIR, SETTINGS_PATH, app_dir
from PyQt6.QtWidgets import (
    QApplication, QMainWindow, QWidget,
    QVBoxLayout, QHBoxLayout,
    QLineEdit, QPushButton, QRadioButton, QButtonGroup,
    QTableWidget, QTableWidgetItem, QHeaderView,
    QLabel, QStatusBar, QComboBox, QFileDialog, QCheckBox, QColorDialog,
    QTabWidget, QStackedWidget, QDialog, QFormLayout, QGridLayout, QDialogButtonBox, QMessageBox,
    QFrame, QListWidget, QListWidgetItem, QSpinBox, QFontComboBox, QTextBrowser,
    QGroupBox, QProgressBar, QScrollArea, QGraphicsOpacityEffect, QSlider,
)
from PyQt6.QtCore import Qt, QThread, pyqtSignal, QTimer, QEvent, QUrl, QPropertyAnimation, QEasingCurve
from PyQt6.QtGui import QFont, QColor, QBrush, QShortcut, QKeySequence, QFontDatabase
from PyQt6.QtMultimedia import QAudioOutput, QMediaPlayer
from qfluentwidgets import (
    FluentIcon,
    NavigationInterface,
    NavigationItemPosition,
    Theme,
    setTheme,
)

# ------------------------------------------------------------------ Settings

_DEFAULTS = {
    "font_family":      "LXGW WenKai",
    "font_size":        11,
    "wc_font_path":     str(app_dir() / "assets" / "fonts" / "LXGWWenKai-Regular.ttf"),
    "theme":            "dark",
    "ctx_len":          40,
    "cross_line":       False,
    "pos_idx":          0,
    "window_width":     1150,
    "window_height":    680,
    "search_col_widths": [110, 170, 78, 260, 90],
    "yomitan_zh_dict":  "",
    "ui_language":      "zh",
    "keyword_color":    "",
    "custom_font_paths": [],
    "playback_rate":    1.0,
    "show_furigana":    False,
    "furigana_mode":    "kanji",
    "song_pitch_semitones": 0,
    "song_lyric_font_family": "Klee One",
    "song_lyric_fallback_font_family": "LXGW WenKai",
    "song_lyric_font_size": 18,
    "song_furigana_font_size": 9,
    "song_lyric_line_spacing": 8,
    "song_lyric_letter_spacing": 1,
}

_BUNDLED_FONT_FILES = [
    app_dir() / "assets" / "fonts" / "KleeOne-SemiBold.ttf",
    app_dir() / "assets" / "fonts" / "LXGWWenKai-Regular.ttf",
]
_LOADED_FONT_PATHS: set[str] = set()

# 主题相关强调色（关键词 / 词元）
_THEME_COLORS = {
    "dark":  {"keyword": "#67e8f9", "lemma": "#5dade2", "wc_bg": "#1e1e1e"},
    "light": {"keyword": "#0891b2", "lemma": "#1a6fa8", "wc_bg": "#ffffff"},
}

_UI_LANGS = {
    "zh": "中文",
    "ja": "日本語",
}

_UI_TEXT = {
    "zh": {
        "settings_title": "设置",
        "ui_language": "界面语言：",
        "font_family": "界面字体：",
        "recommended_font": "推荐字体：",
        "import_font": "导入字体…",
        "font_file_filter": "字体文件 (*.ttf *.otf *.ttc);;所有文件 (*)",
        "keyword_color": "关键词颜色：",
        "choose_color": "选择颜色",
        "reset_default": "恢复默认",
        "font_size": "字体大小：",
        "wc_font": "词云字体：",
        "theme": "界面主题：",
        "theme_dark": "深色",
        "theme_light": "浅色",
        "yomitan_dict": "Yomitan 中文词典：",
        "browse": "浏览…",
        "search_placeholder": "输入检索词后按 Enter …",
        "search_button": "搜索",
        "surface": "表面形",
        "lemma": "词元",
        "filter_open": "▼  筛选条件",
        "filter_closed": "▶  筛选条件",
        "artist": "歌手",
        "pos": "词性",
        "context": "上下文",
        "cross_line": "跨行上下文",
        "jp_only": "仅日文",
        "dedup": "折叠重复行",
        "results": "结果：0 件",
        "audio_repair": "音频修复",
        "audio_reorg": "音频整理",
        "audio_reorg_hint": "整理全曲音频到 raw/audio/{歌手}/",
        "add_song": "添加歌曲",
        "export_csv": "导出 CSV",
        "table_play": "▶",
        "table_artist": "歌手",
        "table_song": "曲名",
        "table_time": "时刻",
        "table_left": "左文脉",
        "table_keyword": "关键词",
        "table_right": "右文脉",
        "loop": "单行循环",
        "playback_speed": "倍速",
        "nav_search": "检索",
        "nav_stats": "词频",
        "nav_report": "报告",
        "nav_songs": "曲库",
        "nav_import": "导入",
        "nav_anki": "Anki",
        "nav_dict": "词典",
        "nav_settings": "设置",
        "ready": "准备就绪 — 双击结果行播放音频",
        "app_title": "JPOP 语料库 KWIC 检索",
        "all_artists": "所有歌手",
        "filter_search": "筛选搜索…",
        "select_all_toggle": "全选 / 取消",
        "selected_count": "{n} 项已选",
        "results_count": "结果：{n} 件",
        "search_loading": "搜索中：{keywords} …",
        "search_done": "完成 — 找到 {n} 件",
        "stats_load": "读取",
        "wordcloud": "词云图",
        "stats_title": "词频统计",
        "stats_subtitle": "按歌手、词性和语言筛选词元，双击词元可回到检索页。",
        "stats_filters": "筛选",
        "stats_results_title": "词频列表",
        "stats_empty": "点击“读取”后显示词频结果。",
        "stats_headers": ["词元", "词性", "JLPT", "出现次数", "出现曲数", "表面形例"],
        "stats_hint": "※ 双击词元 → 自动跳到检索页搜索",
        "stats_loading": "词频读取中 …",
        "stats_count": "{n} 个词",
        "stats_done": "词频读取完成 — {n} 个词",
        "stats_jlpt_cached": "JLPT：已缓存 {cached} 个，正在获取 {uncached} 个…",
        "stats_jlpt_all_cached": "JLPT：全部 {cached} 个已缓存",
        "stats_jlpt_done": "词频读取完成 — {n} 个词  |  JLPT 获取完成",
        "tooltip_search": "KWIC 检索",
        "tooltip_stats": "词频统计",
        "tooltip_report": "语料统计报告",
        "tooltip_songs": "曲库管理",
        "tooltip_import": "添加歌曲",
        "tooltip_anki": "导出 Anki 卡片",
        "tooltip_dict": "管理 Yomitan 词典",
        "tooltip_settings": "字体和界面设置",
        "nav_import_page": "导入歌曲",
        "report_generate": "生成报告",
        "report_export_txt": "导出 TXT",
        "report_empty": "点击“生成报告”后查看语料统计。",
        "report_loading": "计算中 …",
        "report_status": "{songs} 曲 / {tokens} tokens",
        "report_filter": "筛选",
        "report_overview": "总览",
        "report_diversity": "词汇多样性",
        "report_pos": "词性分布",
        "report_coverage": "高频词覆盖",
        "report_top_words": "高频词元 Top 20",
        "report_song_count": "曲数",
        "report_line_count": "歌词行数",
        "report_token_count": "Token 数",
        "report_type_count": "Type 数",
        "report_content_tokens": "实词 token，去除标点",
        "report_unique_lemma": "唯一 lemma 数",
        "report_sttr_note": "标准化 TTR",
        "report_hapax": "Hapax 比率",
        "report_hapax_note": "{count} 个词只出现 1 次",
        "report_avg_song": "平均每曲词汇量",
        "report_avg_line": "平均每行 token",
        "report_all_artists": "全部歌手",
        "report_table_pos": ["词性", "和名", "Token 数", "比率", "Type 数"],
        "report_table_coverage": ["Top N", "覆盖率"],
        "report_table_top": ["排名", "词元", "出现次数"],
        "import_title": "导入歌曲",
        "import_subtitle": "选择音频文件后，程序会读取标题、歌手等标签并加入语料库。",
        "import_pick": "选择音频文件…",
        "import_drop_hint": "支持 FLAC / MP3 / WAV / M4A，可一次选择多首歌",
        "import_empty": "还没有选择文件",
        "import_ready": "{valid} / {total} 个文件可导入",
        "import_skip_hint": "无法读取标题或歌手标签的文件会跳过。",
        "import_start": "开始导入",
        "cancel": "取消",
        "import_audio_title": "选择音频文件",
        "import_audio_filter": "音频文件 (*.flac *.mp3 *.wav *.m4a);;所有文件 (*)",
    },
    "ja": {
        "settings_title": "設定",
        "ui_language": "表示言語：",
        "font_family": "画面フォント：",
        "recommended_font": "おすすめフォント：",
        "import_font": "フォントを取り込む…",
        "font_file_filter": "フォントファイル (*.ttf *.otf *.ttc);;すべてのファイル (*)",
        "keyword_color": "キーワード色：",
        "choose_color": "色を選択",
        "reset_default": "標準に戻す",
        "font_size": "フォントサイズ：",
        "wc_font": "ワードクラウド用フォント：",
        "theme": "テーマ：",
        "theme_dark": "ダーク",
        "theme_light": "ライト",
        "yomitan_dict": "Yomitan 中国語辞書：",
        "browse": "参照…",
        "search_placeholder": "検索語を入力して Enter …",
        "search_button": "検索",
        "surface": "表層形",
        "lemma": "語元",
        "filter_open": "▼  絞り込み条件",
        "filter_closed": "▶  絞り込み条件",
        "artist": "歌手",
        "pos": "品詞",
        "context": "文脈",
        "cross_line": "行をまたぐ文脈",
        "jp_only": "日本語のみ",
        "dedup": "重複行を折りたたむ",
        "results": "結果：0 件",
        "audio_repair": "音声修復",
        "audio_reorg": "音声整理",
        "audio_reorg_hint": "全曲の音声ファイルを raw/audio/{歌手}/ に整理します",
        "add_song": "曲を追加",
        "export_csv": "CSV 出力",
        "table_play": "▶",
        "table_artist": "歌手",
        "table_song": "曲名",
        "table_time": "時刻",
        "table_left": "左文脈",
        "table_keyword": "キーワード",
        "table_right": "右文脈",
        "loop": "単行ループ",
        "playback_speed": "速度",
        "nav_search": "検索",
        "nav_stats": "語頻度",
        "nav_report": "レポート",
        "nav_songs": "曲庫",
        "nav_import": "取り込み",
        "nav_anki": "Anki",
        "nav_dict": "辞書",
        "nav_settings": "設定",
        "ready": "準備完了 — 結果行をダブルクリックすると音声を再生します",
        "app_title": "JPOP 語料庫 KWIC 検索",
        "all_artists": "すべての歌手",
        "filter_search": "絞り込み検索…",
        "select_all_toggle": "すべて選択 / 解除",
        "selected_count": "{n} 件選択中",
        "results_count": "結果：{n} 件",
        "search_loading": "検索中：{keywords} …",
        "search_done": "完了 — {n} 件見つかりました",
        "stats_load": "読み込む",
        "wordcloud": "ワードクラウド",
        "stats_title": "語頻度統計",
        "stats_subtitle": "歌手、品詞、言語で語元を絞り込み、語元をダブルクリックすると検索へ移動します。",
        "stats_filters": "絞り込み",
        "stats_results_title": "語頻度リスト",
        "stats_empty": "「読み込む」を押すと語頻度を表示します。",
        "stats_headers": ["語元", "品詞", "JLPT", "出現回数", "出現曲数", "表面形例"],
        "stats_hint": "※ 語元をダブルクリック → 検索タブで自動検索",
        "stats_loading": "語頻度を読み込み中 …",
        "stats_count": "{n} 語",
        "stats_done": "語頻度読み込み完了 — {n} 語",
        "stats_jlpt_cached": "JLPT: {cached} キャッシュ済み, {uncached} 取得中…",
        "stats_jlpt_all_cached": "JLPT 全 {cached} 語 キャッシュ済み",
        "stats_jlpt_done": "語頻度読み込み完了 — {n} 語  |  JLPT 取得完了",
        "tooltip_search": "KWIC 検索",
        "tooltip_stats": "語頻度統計",
        "tooltip_report": "語料統計レポート",
        "tooltip_songs": "曲庫管理",
        "tooltip_import": "曲を追加",
        "tooltip_anki": "Anki カード出力",
        "tooltip_dict": "Yomitan 辞書を管理",
        "tooltip_settings": "フォントと表示設定",
        "nav_import_page": "曲を取り込む",
        "report_generate": "レポート生成",
        "report_export_txt": "TXT 出力",
        "report_empty": "「レポート生成」を押すと語料統計を表示します。",
        "report_loading": "計算中 …",
        "report_status": "{songs} 曲 / {tokens} tokens",
        "report_filter": "フィルター",
        "report_overview": "概要",
        "report_diversity": "語彙多様性",
        "report_pos": "品詞分布",
        "report_coverage": "高頻度語のカバー率",
        "report_top_words": "高頻度語元 Top 20",
        "report_song_count": "曲数",
        "report_line_count": "歌詞行数",
        "report_token_count": "Token 数",
        "report_type_count": "Type 数",
        "report_content_tokens": "実語 token、句読点除外",
        "report_unique_lemma": "ユニーク lemma 数",
        "report_sttr_note": "標準化 TTR",
        "report_hapax": "Hapax 比率",
        "report_hapax_note": "{count} 語が 1 回のみ出現",
        "report_avg_song": "曲あたり語彙量",
        "report_avg_line": "行あたり token",
        "report_all_artists": "すべての歌手",
        "report_table_pos": ["品詞", "和名", "Token 数", "比率", "Type 数"],
        "report_table_coverage": ["Top N", "カバー率"],
        "report_table_top": ["順位", "語元", "出現回数"],
        "import_title": "曲を取り込む",
        "import_subtitle": "音声ファイルを選択すると、タイトルや歌手などのタグを読み取り語料庫へ追加します。",
        "import_pick": "音声ファイルを選択…",
        "import_drop_hint": "FLAC / MP3 / WAV / M4A に対応、複数選択できます",
        "import_empty": "ファイルが選択されていません",
        "import_ready": "{valid} / {total} 件取り込み可能",
        "import_skip_hint": "タイトルまたは歌手タグを読み取れないファイルはスキップします。",
        "import_start": "取り込み開始",
        "cancel": "キャンセル",
        "import_audio_title": "音声ファイルを選択",
        "import_audio_filter": "音声ファイル (*.flac *.mp3 *.wav *.m4a);;すべてのファイル (*)",
    },
}

_POS_OPTIONS_BY_LANG = {
    "zh": [
        ("词性：全部", ""),
        ("名词 NOUN", "NOUN"),
        ("动词 VERB", "VERB"),
        ("形容词 ADJ", "ADJ"),
        ("副词 ADV", "ADV"),
        ("助动词 AUX", "AUX"),
        ("代名词 PRON", "PRON"),
        ("专有名词 PROPN", "PROPN"),
        ("感叹词 INTJ", "INTJ"),
        ("助词 ADP", "ADP"),
        ("连词 CCONJ", "CCONJ"),
    ],
    "ja": [
        ("品詞：すべて", ""),
        ("名詞 NOUN", "NOUN"),
        ("動詞 VERB", "VERB"),
        ("形容詞 ADJ", "ADJ"),
        ("副詞 ADV", "ADV"),
        ("助動詞 AUX", "AUX"),
        ("代名詞 PRON", "PRON"),
        ("固有名詞 PROPN", "PROPN"),
        ("感動詞 INTJ", "INTJ"),
        ("助詞 ADP", "ADP"),
        ("接続詞 CCONJ", "CCONJ"),
    ],
}


def ui_text(key: str, lang: str | None = None) -> str:
    lang = lang or load_settings().get("ui_language", "zh")
    return _UI_TEXT.get(lang, _UI_TEXT["zh"]).get(key, _UI_TEXT["zh"].get(key, key))


def pos_options(lang: str | None = None):
    lang = lang or load_settings().get("ui_language", "zh")
    return _POS_OPTIONS_BY_LANG.get(lang, _POS_OPTIONS_BY_LANG["zh"])

_QSS_POLISH = """
    QWidget {
        font-family: "Microsoft YaHei UI", "Yu Gothic UI", "Segoe UI";
        letter-spacing: 0px;
    }
    QLabel {
        font-weight: 400;
    }
    QLabel#MutedLabel {
        font-size: 12px;
    }
    QFrame#TopPanel, QFrame#FilterPanel, QFrame#ActionPanel, QGroupBox {
        border-radius: 8px;
    }
    QLineEdit, QTextEdit, QTextBrowser, QSpinBox, QComboBox {
        border-radius: 8px;
        padding: 5px 9px;
        min-height: 26px;
        font-weight: 400;
    }
    QPushButton {
        border-radius: 8px;
        padding: 5px 12px;
        min-height: 26px;
        font-weight: 500;
    }
    QLineEdit#SearchInput {
        font-weight: 500;
    }
    QPushButton[role="primary"] {
        font-weight: 600;
    }
    QPushButton[speed="true"] {
        min-height: 24px;
        padding: 4px 9px;
        border-radius: 7px;
        font-weight: 600;
    }
    QFrame#SpeedPanel {
        border-radius: 8px;
        background: transparent;
    }
    QRadioButton, QCheckBox {
        spacing: 9px;
        font-weight: 400;
    }
    QRadioButton::indicator, QCheckBox::indicator {
        width: 18px;
        height: 18px;
    }
    QTableWidget::item {
        padding: 5px 9px;
    }
    QHeaderView::section {
        padding: 6px 9px;
    }
"""

_QSS_DARK = """
    * { outline: none; }
    QMainWindow, QDialog {
        background-color: #1f2125;
        color: #e6e8eb;
    }
    QWidget {
        background-color: #1f2125;
        color: #e6e8eb;
        selection-background-color: #2f80ed;
        selection-color: #ffffff;
    }
    QFrame#TopPanel, QFrame#FilterPanel, QFrame#ActionPanel, QGroupBox {
        background-color: #292c31;
        border: 1px solid #3a3f46;
        border-radius: 8px;
    }
    QGroupBox {
        margin-top: 13px;
        padding: 12px 10px 10px 10px;
        font-weight: 600;
    }
    QGroupBox::title {
        subcontrol-origin: margin;
        left: 10px;
        padding: 0 6px;
        color: #aeb7c2;
        background: #292c31;
    }
    QLabel { color: #d9dde3; background: transparent; }
    QLabel#MutedLabel { color: #8f99a6; }
    QLineEdit, QTextEdit, QTextBrowser, QSpinBox, QComboBox {
        background-color: #22252a;
        color: #edf0f3;
        border: 1px solid #414852;
        border-radius: 6px;
        padding: 6px 9px;
        min-height: 24px;
    }
    QLineEdit:focus, QTextEdit:focus, QSpinBox:focus, QComboBox:focus {
        border: 1px solid #3b8cff;
        background-color: #242932;
    }
    QLineEdit::placeholder { color: #777f8c; }
    QComboBox::drop-down {
        border: none;
        width: 24px;
    }
    QComboBox QAbstractItemView, QListWidget {
        background-color: #25282e;
        color: #edf0f3;
        border: 1px solid #424851;
        selection-background-color: #2f80ed;
        selection-color: #ffffff;
    }
    QPushButton {
        background-color: #30343b;
        color: #edf0f3;
        border: 1px solid #48505b;
        padding: 6px 13px;
        border-radius: 6px;
        min-height: 24px;
    }
    QPushButton:hover {
        background-color: #3a4049;
        border-color: #5a6471;
    }
    QPushButton:pressed { background-color: #252a31; }
    QPushButton:disabled {
        color: #707783;
        background-color: #25282d;
        border-color: #333840;
    }
    QPushButton[role="primary"] {
        background-color: #2f80ed;
        border-color: #2f80ed;
        color: #ffffff;
        font-weight: 600;
    }
    QPushButton[role="primary"]:hover { background-color: #4592ff; }
    QPushButton[role="subtle"] {
        background-color: transparent;
        border-color: #3d444d;
        color: #b8c2ce;
    }
    QPushButton[speed="true"]:checked {
        background-color: #2f80ed;
        border-color: #2f80ed;
        color: #ffffff;
    }
    QPushButton[role="danger"] {
        background-color: #513034;
        border-color: #8a3f46;
        color: #ffd9dc;
    }
    QRadioButton, QCheckBox {
        color: #d9dde3;
        spacing: 7px;
        background: transparent;
    }
    QRadioButton::indicator, QCheckBox::indicator {
        width: 15px;
        height: 15px;
    }
    QCheckBox::indicator {
        border: 1px solid #566170;
        border-radius: 5px;
        background-color: #22262c;
    }
    QCheckBox::indicator:hover {
        border-color: #748094;
        background-color: #272c34;
    }
    QCheckBox::indicator:checked {
        border-color: #3b8cff;
        background-color: #2f80ed;
    }
    QRadioButton::indicator {
        border: 1px solid #566170;
        border-radius: 8px;
        background-color: #22262c;
    }
    QRadioButton::indicator:checked {
        border: 5px solid #2f80ed;
        background-color: #ffffff;
    }
    QTabWidget::pane {
        border: none;
        top: -1px;
    }
    QTabBar::tab {
        background-color: transparent;
        color: #aeb7c2;
        padding: 9px 16px;
        border: none;
        border-bottom: 2px solid transparent;
    }
    QTabBar::tab:hover { color: #ffffff; background-color: #292c31; }
    QTabBar::tab:selected {
        color: #ffffff;
        border-bottom: 2px solid #2f80ed;
        font-weight: 600;
    }
    QMenuBar {
        background-color: #1a1c20;
        color: #d9dde3;
        padding: 3px 6px;
        border-bottom: 1px solid #31363d;
    }
    QMenuBar::item { padding: 5px 9px; border-radius: 5px; }
    QMenuBar::item:selected { background-color: #2d3239; }
    QMenu {
        background-color: #25282e;
        color: #e6e8eb;
        border: 1px solid #3c424b;
        padding: 5px;
    }
    QMenu::item { padding: 6px 22px; border-radius: 4px; }
    QMenu::item:selected { background-color: #2f80ed; color: #ffffff; }
    QTableWidget {
        background-color: #23262b;
        alternate-background-color: #282c32;
        gridline-color: #353b44;
        border: 1px solid #3a3f46;
        border-radius: 8px;
        color: #e8ebef;
    }
    QTableWidget::item {
        padding: 5px 9px;
        border: none;
    }
    QTableWidget::item:selected {
        background-color: #245fbd;
        color: #ffffff;
    }
    QHeaderView::section {
        background-color: #2b2f36;
        color: #bfc8d4;
        padding: 7px 9px;
        border: none;
        border-right: 1px solid #3a4049;
        border-bottom: 1px solid #3a4049;
        font-weight: 600;
    }
    QProgressBar {
        background-color: #25282d;
        color: #ffffff;
        border: 1px solid #3f4650;
        border-radius: 6px;
        text-align: center;
        min-height: 16px;
    }
    QProgressBar::chunk {
        background-color: #2f80ed;
        border-radius: 5px;
    }
    QStatusBar {
        background-color: #1a1c20;
        color: #9da7b4;
        border-top: 1px solid #31363d;
    }
    QScrollBar:vertical {
        background: #1f2125;
        width: 11px;
        margin: 0;
    }
    QScrollBar::handle:vertical {
        background: #4a515c;
        border-radius: 5px;
        min-height: 30px;
    }
    QScrollBar::handle:vertical:hover { background: #68717f; }
    QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical { height: 0; }
"""

_QSS_LIGHT = """
    * { outline: none; }
    QMainWindow, QDialog {
        background-color: #f5f7fa;
        color: #20242a;
    }
    QWidget {
        background-color: #f5f7fa;
        color: #20242a;
        selection-background-color: #1f6fd1;
        selection-color: #ffffff;
    }
    QFrame#TopPanel, QFrame#FilterPanel, QFrame#ActionPanel, QGroupBox {
        background-color: #ffffff;
        border: 1px solid #d9dee6;
        border-radius: 8px;
    }
    QGroupBox {
        margin-top: 13px;
        padding: 12px 10px 10px 10px;
        font-weight: 600;
    }
    QGroupBox::title {
        subcontrol-origin: margin;
        left: 10px;
        padding: 0 6px;
        color: #5d6876;
        background: #ffffff;
    }
    QLabel { color: #20242a; background: transparent; }
    QLabel#MutedLabel { color: #6d7785; }
    QLineEdit, QTextEdit, QTextBrowser, QSpinBox, QComboBox {
        background-color: #ffffff;
        color: #20242a;
        border: 1px solid #cbd3dd;
        border-radius: 6px;
        padding: 6px 9px;
        min-height: 24px;
    }
    QLineEdit:focus, QTextEdit:focus, QSpinBox:focus, QComboBox:focus {
        border: 1px solid #1f6fd1;
        background-color: #ffffff;
    }
    QLineEdit::placeholder { color: #8a94a3; }
    QComboBox::drop-down { border: none; width: 24px; }
    QComboBox QAbstractItemView, QListWidget {
        background-color: #ffffff;
        color: #20242a;
        border: 1px solid #cbd3dd;
        selection-background-color: #1f6fd1;
        selection-color: #ffffff;
    }
    QPushButton {
        background-color: #ffffff;
        color: #20242a;
        border: 1px solid #cbd3dd;
        padding: 6px 13px;
        border-radius: 6px;
        min-height: 24px;
    }
    QPushButton:hover {
        background-color: #eef3f8;
        border-color: #b6c1ce;
    }
    QPushButton:pressed { background-color: #e4ebf3; }
    QPushButton:disabled {
        color: #a2abb7;
        background-color: #eef1f5;
        border-color: #d9dee6;
    }
    QPushButton[role="primary"] {
        background-color: #1f6fd1;
        border-color: #1f6fd1;
        color: #ffffff;
        font-weight: 600;
    }
    QPushButton[role="primary"]:hover { background-color: #2f80ed; }
    QPushButton[role="subtle"] {
        background-color: transparent;
        border-color: #d0d7e0;
        color: #4c5968;
    }
    QPushButton[speed="true"]:checked {
        background-color: #1f6fd1;
        border-color: #1f6fd1;
        color: #ffffff;
    }
    QPushButton[role="danger"] {
        background-color: #fff1f2;
        border-color: #e6a5ab;
        color: #9c2932;
    }
    QRadioButton, QCheckBox {
        color: #20242a;
        spacing: 7px;
        background: transparent;
    }
    QRadioButton::indicator, QCheckBox::indicator {
        width: 15px;
        height: 15px;
    }
    QCheckBox::indicator {
        border: 1px solid #aeb8c5;
        border-radius: 5px;
        background-color: #ffffff;
    }
    QCheckBox::indicator:hover {
        border-color: #7f8da1;
        background-color: #f5f8fb;
    }
    QCheckBox::indicator:checked {
        border-color: #1f6fd1;
        background-color: #1f6fd1;
    }
    QRadioButton::indicator {
        border: 1px solid #aeb8c5;
        border-radius: 8px;
        background-color: #ffffff;
    }
    QRadioButton::indicator:checked {
        border: 5px solid #1f6fd1;
        background-color: #ffffff;
    }
    QTabWidget::pane { border: none; top: -1px; }
    QTabBar::tab {
        background-color: transparent;
        color: #5f6b7a;
        padding: 9px 16px;
        border: none;
        border-bottom: 2px solid transparent;
    }
    QTabBar::tab:hover { color: #20242a; background-color: #edf2f7; }
    QTabBar::tab:selected {
        color: #20242a;
        border-bottom: 2px solid #1f6fd1;
        font-weight: 600;
    }
    QMenuBar {
        background-color: #ffffff;
        color: #20242a;
        padding: 3px 6px;
        border-bottom: 1px solid #d9dee6;
    }
    QMenuBar::item { padding: 5px 9px; border-radius: 5px; }
    QMenuBar::item:selected { background-color: #edf2f7; }
    QMenu {
        background-color: #ffffff;
        color: #20242a;
        border: 1px solid #d9dee6;
        padding: 5px;
    }
    QMenu::item { padding: 6px 22px; border-radius: 4px; }
    QMenu::item:selected { background-color: #1f6fd1; color: #ffffff; }
    QTableWidget {
        background-color: #ffffff;
        alternate-background-color: #f3f6fa;
        gridline-color: #e1e6ed;
        border: 1px solid #d9dee6;
        border-radius: 8px;
        color: #20242a;
    }
    QTableWidget::item {
        padding: 5px 9px;
        border: none;
    }
    QTableWidget::item:selected {
        background-color: #d8e9ff;
        color: #10233d;
    }
    QHeaderView::section {
        background-color: #f0f4f8;
        color: #4d5968;
        padding: 7px 9px;
        border: none;
        border-right: 1px solid #d9dee6;
        border-bottom: 1px solid #d9dee6;
        font-weight: 600;
    }
    QProgressBar {
        background-color: #edf1f6;
        color: #20242a;
        border: 1px solid #d9dee6;
        border-radius: 6px;
        text-align: center;
        min-height: 16px;
    }
    QProgressBar::chunk {
        background-color: #1f6fd1;
        border-radius: 5px;
    }
    QStatusBar {
        background-color: #ffffff;
        color: #5f6b7a;
        border-top: 1px solid #d9dee6;
    }
    QScrollBar:vertical {
        background: #f5f7fa;
        width: 11px;
        margin: 0;
    }
    QScrollBar::handle:vertical {
        background: #b8c2cf;
        border-radius: 5px;
        min-height: 30px;
    }
    QScrollBar::handle:vertical:hover { background: #8f9baa; }
    QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical { height: 0; }
"""

def apply_theme(app, theme: str):
    theme = "light" if theme == "light" else "dark"
    app.setProperty("jpopTheme", theme)
    setTheme(Theme.DARK if theme == "dark" else Theme.LIGHT)
    app.setStyleSheet((_QSS_DARK if theme == "dark" else _QSS_LIGHT) + _QSS_POLISH)

    # Qt stylesheets do not reach colors painted manually or cached by tool pages.
    # Notify each top-level surface so embedded pages can refresh those colors too.
    for widget in list(app.topLevelWidgets()):
        refresh = getattr(widget, "refresh_theme", None)
        if callable(refresh):
            refresh(theme)


def fade_in_widget(widget: QWidget, duration: int = 150, start_opacity: float = 0.72):
    effect = QGraphicsOpacityEffect(widget)
    effect.setOpacity(start_opacity)
    widget.setGraphicsEffect(effect)
    anim = QPropertyAnimation(effect, b"opacity", widget)
    anim.setDuration(duration)
    anim.setStartValue(start_opacity)
    anim.setEndValue(1.0)
    anim.setEasingCurve(QEasingCurve.Type.OutCubic)

    def _finish():
        widget.setGraphicsEffect(None)

    anim.finished.connect(_finish)
    widget._fade_anim = anim
    anim.start()

def set_button_role(button: QPushButton, role: str):
    button.setProperty("role", role)
    button.style().unpolish(button)
    button.style().polish(button)

def tune_table(table: QTableWidget, row_height: int = 36):
    table.setShowGrid(False)
    table.setAlternatingRowColors(True)
    table.setWordWrap(False)
    table.verticalHeader().setVisible(False)
    table.verticalHeader().setDefaultSectionSize(row_height)
    table.horizontalHeader().setHighlightSections(False)

def load_settings() -> dict:
    if SETTINGS_PATH.exists():
        try:
            with open(SETTINGS_PATH, encoding="utf-8") as f:
                return {**_DEFAULTS, **json.load(f)}
        except Exception:
            pass
    return dict(_DEFAULTS)

def save_settings(s: dict):
    with open(SETTINGS_PATH, "w", encoding="utf-8") as f:
        json.dump(s, f, ensure_ascii=False, indent=2)

def theme_color(theme: str, key: str) -> str:
    return _THEME_COLORS.get(theme, _THEME_COLORS["dark"]).get(key, "")

def keyword_color_for(cfg: dict) -> str:
    saved = (cfg.get("keyword_color") or "").strip()
    if QColor(saved).isValid():
        return saved
    return theme_color(cfg.get("theme", "dark"), "keyword")

def load_custom_fonts(paths: list[str]) -> list[str]:
    families: list[str] = []
    for path in [*_BUNDLED_FONT_FILES, *(paths or [])]:
        if not path:
            continue
        font_path = pathlib.Path(path)
        if not font_path.exists():
            continue
        try:
            resolved = str(font_path.resolve())
        except Exception:
            resolved = str(font_path)
        if resolved in _LOADED_FONT_PATHS:
            continue
        font_id = QFontDatabase.addApplicationFont(str(font_path))
        if font_id < 0:
            continue
        _LOADED_FONT_PATHS.add(resolved)
        families.extend(QFontDatabase.applicationFontFamilies(font_id))
    return families

_RECOMMENDED_UI_FONTS = [
    "LXGW WenKai",
    "Klee One",
    "Microsoft YaHei UI",
    "Segoe UI",
    "Yu Gothic UI",
    "Meiryo UI",
    "Noto Sans CJK SC",
    "Noto Sans CJK JP",
    "Source Han Sans SC",
    "Source Han Sans JP",
    "BIZ UDPGothic",
]

_PLAYBACK_RATES = [0.5, 0.75, 1.0, 1.25, 1.5]

def normalize_playback_rate(value) -> float:
    try:
        rate = float(value)
    except (TypeError, ValueError):
        rate = 1.0
    return round(max(0.5, min(rate, 1.5)), 2)

def playback_rate_label(rate: float) -> str:
    rate = normalize_playback_rate(rate)
    return f"{rate:.1f}x" if rate in (0.5, 1.0, 1.5) else f"{rate:g}x"


class PlaybackSpeedControl(QWidget):
    rateChanged = pyqtSignal(float)

    def __init__(self, rate: float = 1.0, parent=None):
        super().__init__(parent)
        self._rate = normalize_playback_rate(rate)
        self._expanded = False
        self._anim = None

        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(6)

        self.button = QPushButton(playback_rate_label(self._rate))
        self.button.setCheckable(True)
        self.button.setProperty("speed", True)
        self.button.setMinimumWidth(54)
        set_button_role(self.button, "subtle")
        self.button.clicked.connect(self.toggle)
        layout.addWidget(self.button)

        self.panel = QFrame()
        self.panel.setObjectName("SpeedPanel")
        self.panel.setMaximumWidth(0)
        panel_layout = QHBoxLayout(self.panel)
        panel_layout.setContentsMargins(8, 0, 8, 0)
        panel_layout.setSpacing(8)
        self.slider = QSlider(Qt.Orientation.Horizontal)
        self.slider.setRange(50, 150)
        self.slider.setSingleStep(5)
        self.slider.setPageStep(25)
        self.slider.setTickInterval(25)
        self.slider.setTickPosition(QSlider.TickPosition.TicksBelow)
        self.slider.setFixedWidth(130)
        self.slider.valueChanged.connect(self._on_slider_value)
        self.label = QLabel(playback_rate_label(self._rate))
        self.label.setMinimumWidth(42)
        panel_layout.addWidget(self.slider)
        panel_layout.addWidget(self.label)
        layout.addWidget(self.panel)
        self.setRate(self._rate, emit=False)

    def rate(self) -> float:
        return self._rate

    def setRate(self, rate: float, emit: bool = False):
        self._rate = normalize_playback_rate(rate)
        value = int(round(self._rate * 100))
        self.slider.blockSignals(True)
        self.slider.setValue(value)
        self.slider.blockSignals(False)
        text = playback_rate_label(self._rate)
        self.button.setText(text)
        self.label.setText(text)
        if emit:
            self.rateChanged.emit(self._rate)

    def toggle(self):
        self.setExpanded(not self._expanded)

    def setExpanded(self, expanded: bool):
        self._expanded = expanded
        self.button.setChecked(expanded)
        start = self.panel.maximumWidth()
        end = 194 if expanded else 0
        self._anim = QPropertyAnimation(self.panel, b"maximumWidth", self)
        self._anim.setDuration(170)
        self._anim.setStartValue(start)
        self._anim.setEndValue(end)
        self._anim.setEasingCurve(QEasingCurve.Type.OutCubic)
        self._anim.start()

    def _on_slider_value(self, value: int):
        rate = normalize_playback_rate(value / 100)
        self._rate = rate
        text = playback_rate_label(rate)
        self.button.setText(text)
        self.label.setText(text)
        self.rateChanged.emit(rate)


LRC_LINE_RE   = re.compile(r"^\[(\d+):(\d+\.\d+)\](.*)")
SKIP_KEYWORDS = ["作词", "作曲", "编曲", "作詞", "作曲", "編曲"]

# 含有平假名、片假名或汉字 → 视为日语词
_JP_RE = re.compile(r'[ぁ-んァ-ヶｦ-ｿ\u4E00-\u9FFF\u3400-\u4DBF]')

POS_OPTIONS = _POS_OPTIONS_BY_LANG["ja"]


# ------------------------------------------------------------------ FontSettingsDialog

class FontSettingsDialog(QDialog):
    themePreviewRequested = pyqtSignal(str)
    def __init__(self, family: str, size: int, wc_font_path: str, theme: str,
                 yomitan_zh_dict: str = "", ui_language: str = "zh",
                 keyword_color: str = "", custom_font_paths: list[str] | None = None,
                 parent=None):
        super().__init__(parent)
        load_custom_fonts(custom_font_paths or [])
        self.lang = ui_language if ui_language in _UI_LANGS else "zh"
        self._theme = theme
        self._keyword_color_custom = QColor(keyword_color).isValid()
        self._keyword_color = keyword_color if self._keyword_color_custom else theme_color(theme, "keyword")
        self._custom_font_paths = list(custom_font_paths or [])
        self.setWindowTitle(ui_text("settings_title", self.lang))
        self.setMinimumWidth(720)
        self.setMinimumHeight(560)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(20, 18, 20, 18)
        layout.setSpacing(14)

        header = QLabel(ui_text("settings_title", self.lang))
        header.setObjectName("SettingsTitle")
        layout.addWidget(header)

        appearance = QFrame()
        appearance.setObjectName("SettingsPanel")
        form = QFormLayout(appearance)
        form.setContentsMargins(18, 16, 18, 16)
        form.setSpacing(12)
        self.lang_combo = QComboBox()
        for code, label in _UI_LANGS.items():
            self.lang_combo.addItem(label, code)
        self.lang_combo.setCurrentIndex(max(0, self.lang_combo.findData(self.lang)))
        form.addRow(ui_text("ui_language", self.lang), self.lang_combo)

        recommended_row = QHBoxLayout()
        self.recommended_font_combo = QComboBox()
        installed = set(QFontDatabase.families())
        for name in _RECOMMENDED_UI_FONTS:
            if name in installed:
                self.recommended_font_combo.addItem(name, name)
        if self.recommended_font_combo.count() == 0:
            self.recommended_font_combo.addItem(family, family)
        btn_apply_recommended = QPushButton("应用" if self.lang == "zh" else "適用")
        set_button_role(btn_apply_recommended, "subtle")
        btn_apply_recommended.clicked.connect(self._apply_recommended_font)
        recommended_row.addWidget(self.recommended_font_combo, 1)
        recommended_row.addWidget(btn_apply_recommended)
        form.addRow(ui_text("recommended_font", self.lang), recommended_row)

        self.font_combo = QFontComboBox()
        self.font_combo.setCurrentFont(QFont(family))
        font_row = QHBoxLayout()
        btn_import_font = QPushButton(ui_text("import_font", self.lang))
        set_button_role(btn_import_font, "subtle")
        btn_import_font.clicked.connect(self._import_font)
        font_row.addWidget(self.font_combo, 1)
        font_row.addWidget(btn_import_font)
        form.addRow(ui_text("font_family", self.lang), font_row)

        self.size_spin = QSpinBox()
        self.size_spin.setRange(8, 28)
        self.size_spin.setValue(size)
        self.size_spin.setSuffix(" pt")
        form.addRow(ui_text("font_size", self.lang), self.size_spin)

        self.wc_font_combo = QComboBox()
        for name, path in _AVAIL_WC_FONTS:
            self.wc_font_combo.addItem(name, path)
        cur_idx = next((i for i, (_, p) in enumerate(_AVAIL_WC_FONTS) if p == wc_font_path), 0)
        self.wc_font_combo.setCurrentIndex(cur_idx)
        form.addRow(ui_text("wc_font", self.lang), self.wc_font_combo)

        # 主题切换
        theme_row = QHBoxLayout()
        self.rb_dark  = QRadioButton(ui_text("theme_dark", self.lang))
        self.rb_light = QRadioButton(ui_text("theme_light", self.lang))
        (self.rb_dark if theme == "dark" else self.rb_light).setChecked(True)
        theme_row.addWidget(self.rb_dark)
        theme_row.addWidget(self.rb_light)
        theme_row.addStretch()
        form.addRow(ui_text("theme", self.lang), theme_row)
        layout.addWidget(appearance)

        color_panel = QFrame()
        color_panel.setObjectName("SettingsPanel")
        color_form = QFormLayout(color_panel)
        color_form.setContentsMargins(18, 16, 18, 16)
        color_form.setSpacing(12)
        color_row = QHBoxLayout()
        self.keyword_color_preview = QLabel("关键词 / キーワード")
        self.keyword_color_preview.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.keyword_color_preview.setMinimumHeight(34)
        btn_choose_color = QPushButton(ui_text("choose_color", self.lang))
        set_button_role(btn_choose_color, "subtle")
        btn_choose_color.clicked.connect(self._choose_keyword_color)
        btn_reset_color = QPushButton(ui_text("reset_default", self.lang))
        set_button_role(btn_reset_color, "subtle")
        btn_reset_color.clicked.connect(self._reset_keyword_color)
        color_row.addWidget(self.keyword_color_preview, 1)
        color_row.addWidget(btn_choose_color)
        color_row.addWidget(btn_reset_color)
        color_form.addRow(ui_text("keyword_color", self.lang), color_row)
        layout.addWidget(color_panel)

        # Yomitan 中文词典路径
        dict_panel = QFrame()
        dict_panel.setObjectName("SettingsPanel")
        dict_form = QFormLayout(dict_panel)
        dict_form.setContentsMargins(18, 16, 18, 16)
        dict_form.setSpacing(12)
        yomi_row = QHBoxLayout()
        self.yomitan_edit = QLineEdit(yomitan_zh_dict)
        self.yomitan_edit.setPlaceholderText("Yomitan 格式中文词典 zip 路径（用于中文释义）")
        btn_yomi = QPushButton(ui_text("browse", self.lang))
        btn_yomi.setFixedWidth(60)
        btn_yomi.clicked.connect(self._browse_yomitan)
        yomi_row.addWidget(self.yomitan_edit, 1)
        yomi_row.addWidget(btn_yomi)
        dict_form.addRow(ui_text("yomitan_dict", self.lang), yomi_row)
        layout.addWidget(dict_panel)

        self.preview = QLabel("あいうえお ABCDE　夜に駆ける　君の名は")
        self.preview.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.preview.setObjectName("SettingsPreview")
        layout.addWidget(self.preview)
        layout.addStretch()

        btns = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel
        )
        btns.button(QDialogButtonBox.StandardButton.Ok).setText(
            "保存" if self.lang == "zh" else "保存")
        btns.button(QDialogButtonBox.StandardButton.Cancel).setText(
            "取消" if self.lang == "zh" else "キャンセル")
        btns.accepted.connect(self.accept)
        btns.rejected.connect(self.reject)
        layout.addWidget(btns)

        self.font_combo.currentFontChanged.connect(self._update_preview)
        self.size_spin.valueChanged.connect(self._update_preview)
        self.rb_dark.toggled.connect(self._on_theme_toggled)
        self.rb_light.toggled.connect(self._on_theme_toggled)
        self._apply_local_style()
        self._update_preview()

    def _apply_local_style(self):
        c = {
            "bg": "#171a1f", "panel": "#22262d", "border": "#343c48",
            "fg": "#eef3f8", "muted": "#9aa7b4"
        } if self.rb_dark.isChecked() else {
            "bg": "#f5f7fb", "panel": "#ffffff", "border": "#d8e0ea",
            "fg": "#1f2937", "muted": "#64748b"
        }
        self.setStyleSheet(f"""
            QDialog {{ background: {c["bg"]}; color: {c["fg"]}; }}
            QFrame#SettingsPanel {{
                background: {c["panel"]};
                border: 1px solid {c["border"]};
                border-radius: 10px;
            }}
            QLabel#SettingsTitle {{
                color: {c["fg"]};
                font-size: 22px;
                font-weight: 800;
            }}
            QLabel#SettingsPreview {{
                border: 1px solid {c["border"]};
                border-radius: 10px;
                padding: 16px;
                background: {c["panel"]};
                color: {c["fg"]};
            }}
        """)

    def _on_theme_toggled(self, checked: bool):
        if not checked:
            return
        self._update_preview()
        self.themePreviewRequested.emit("dark" if self.rb_dark.isChecked() else "light")

    def _apply_recommended_font(self):
        family = self.recommended_font_combo.currentData()
        if family:
            self.font_combo.setCurrentFont(QFont(family))

    def _import_font(self):
        paths, _ = QFileDialog.getOpenFileNames(
            self, ui_text("import_font", self.lang), "", ui_text("font_file_filter", self.lang)
        )
        for path in paths:
            if path not in self._custom_font_paths:
                self._custom_font_paths.append(path)
            font_id = QFontDatabase.addApplicationFont(path)
            families = QFontDatabase.applicationFontFamilies(font_id) if font_id >= 0 else []
            if families:
                self.font_combo.setCurrentFont(QFont(families[0]))

    def _choose_keyword_color(self):
        color = QColorDialog.getColor(QColor(self._keyword_color), self, ui_text("keyword_color", self.lang))
        if color.isValid():
            self._keyword_color = color.name()
            self._keyword_color_custom = True
            self._update_preview()

    def _reset_keyword_color(self):
        self._keyword_color_custom = False
        self._keyword_color = theme_color("dark" if self.rb_dark.isChecked() else "light", "keyword")
        self._update_preview()

    def _browse_yomitan(self):
        path, _ = QFileDialog.getOpenFileName(
            self, "选择 Yomitan 中文词典 zip", "", "ZIP 文件 (*.zip)")
        if path:
            self.yomitan_edit.setText(path)

    def _update_preview(self):
        dark = self.rb_dark.isChecked()
        theme = "dark" if dark else "light"
        keyword = self._keyword_color if self._keyword_color_custom else theme_color(theme, "keyword")
        bg, fg, border = ("#22262d", "#e8e8e8", "#343c48") if dark else ("#ffffff", "#1a1a1a", "#d8e0ea")
        self.preview.setStyleSheet(
            f"border:1px solid {border}; padding:10px; background:{bg}; color:{fg};"
        )
        self.preview.setFont(QFont(self.font_combo.currentFont().family(),
                                   self.size_spin.value()))
        self.keyword_color_preview.setStyleSheet(
            f"border:1px solid {border}; border-radius:8px; padding:6px 10px;"
            f"background:{bg}; color:{keyword}; font-weight:800;"
        )
        self.keyword_color_preview.setFont(QFont(self.font_combo.currentFont().family(),
                                                self.size_spin.value(), QFont.Weight.Bold))
        self._apply_local_style()

    def result_font(self) -> tuple[str, int, str, str, str, str, str, list[str]]:
        return (
            self.font_combo.currentFont().family(),
            self.size_spin.value(),
            self.wc_font_combo.currentData(),
            "dark" if self.rb_dark.isChecked() else "light",
            self.yomitan_edit.text().strip(),
            self.lang_combo.currentData(),
            self._keyword_color if self._keyword_color_custom and QColor(self._keyword_color).isValid() else "",
            self._custom_font_paths,
        )




# Song management dialogs. moved to dialogs/song_manager.py


class MultiSelectComboBox(QWidget):
    """チェックボックス付き + 絞り込み検索の多重選択コンボ（Excel フィルター風）。"""
    selectionChanged = pyqtSignal()

    def __init__(self, placeholder: str | None = None, parent=None, lang: str | None = None):
        super().__init__(parent)
        self._lang = lang or load_settings().get("ui_language", "zh")
        if placeholder is None:
            placeholder = ui_text("all_artists", self._lang)
        self._placeholder  = placeholder
        self._all_items: list[str] = []
        self._checked:   set[str]  = set()

        from PyQt6.QtWidgets import QSizePolicy as QSP
        self.setSizePolicy(QSP.Policy.Expanding, QSP.Policy.Fixed)
        lay = QHBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        self._btn = QPushButton(placeholder)
        self._btn.setMinimumWidth(100)
        self._btn.setSizePolicy(QSP.Policy.Expanding, QSP.Policy.Fixed)
        self._btn.clicked.connect(self._show_popup)
        lay.addWidget(self._btn)

        # Qt.WindowType.Popup → 外クリックで自動クローズ
        self._popup = QFrame(None, Qt.WindowType.Popup)
        self._popup.setMinimumWidth(240)
        pl = QVBoxLayout(self._popup)
        pl.setContentsMargins(6, 6, 6, 6)
        pl.setSpacing(4)

        self._search = QLineEdit()
        self._search.setPlaceholderText(ui_text("filter_search", self._lang))
        self._search.textChanged.connect(self._filter)
        pl.addWidget(self._search)

        self._all_cb = QCheckBox(ui_text("select_all_toggle", self._lang))
        self._all_cb.setTristate(True)
        self._all_cb.clicked.connect(self._toggle_all)
        pl.addWidget(self._all_cb)

        self._list = QListWidget()
        self._list.setMinimumHeight(160)
        self._list.setMaximumHeight(260)
        self._list.itemChanged.connect(self._on_item_changed)
        pl.addWidget(self._list)

    # ---------- public API ----------

    def setItems(self, items: list[str]):
        """纯字符串列表：label == value。"""
        self._all_items = [(s, s) for s in items]
        self._checked.clear()
        self._update_btn()

    def setItemsLabeled(self, items: list[tuple[str, str]]):
        """(显示标签, 实际值) 列表。"""
        self._all_items = items
        self._checked.clear()
        self._update_btn()

    def selectedValues(self) -> list[str]:
        """空リスト → フィルターなし（すべて）。"""
        return list(self._checked)

    # ---------- popup ----------

    def _show_popup(self):
        pos = self._btn.mapToGlobal(self._btn.rect().bottomLeft())
        self._popup.adjustSize()
        self._popup.move(pos)
        self._search.clear()
        self._rebuild_list(self._all_items)
        self._popup.show()
        self._search.setFocus()

    def _filter(self, text: str):
        kw = text.lower()
        subset = [(lbl, val) for lbl, val in self._all_items if kw in lbl.lower()] if kw else self._all_items
        self._rebuild_list(subset)

    def _rebuild_list(self, items: list[tuple[str, str]]):
        self._list.blockSignals(True)
        self._list.clear()
        for label, value in items:
            li = QListWidgetItem(label)
            li.setData(Qt.ItemDataRole.UserRole, value)
            li.setFlags(Qt.ItemFlag.ItemIsEnabled | Qt.ItemFlag.ItemIsUserCheckable)
            li.setCheckState(
                Qt.CheckState.Checked if value in self._checked
                else Qt.CheckState.Unchecked
            )
            self._list.addItem(li)
        self._list.blockSignals(False)
        self._sync_all_cb()

    # ---------- events ----------

    def _on_item_changed(self, item: QListWidgetItem):
        value = item.data(Qt.ItemDataRole.UserRole)
        if item.checkState() == Qt.CheckState.Checked:
            self._checked.add(value)
        else:
            self._checked.discard(value)
        self._sync_all_cb()
        self._update_btn()
        self.selectionChanged.emit()

    def _toggle_all(self):
        visible = [self._list.item(i) for i in range(self._list.count())]
        all_on  = all(li.checkState() == Qt.CheckState.Checked for li in visible)
        state   = Qt.CheckState.Unchecked if all_on else Qt.CheckState.Checked
        self._list.blockSignals(True)
        for li in visible:
            li.setCheckState(state)
            value = li.data(Qt.ItemDataRole.UserRole)
            (self._checked.add if state == Qt.CheckState.Checked
             else self._checked.discard)(value)
        self._list.blockSignals(False)
        self._sync_all_cb()
        self._update_btn()
        self.selectionChanged.emit()

    def _sync_all_cb(self):
        visible = [self._list.item(i) for i in range(self._list.count())]
        if not visible:
            return
        n = sum(1 for li in visible if li.checkState() == Qt.CheckState.Checked)
        self._all_cb.blockSignals(True)
        self._all_cb.setCheckState(
            Qt.CheckState.Checked if n == len(visible) else
            Qt.CheckState.PartiallyChecked if n > 0 else
            Qt.CheckState.Unchecked
        )
        self._all_cb.blockSignals(False)

    def _update_btn(self):
        n = len(self._checked)
        self._btn.setText(
            self._placeholder if n == 0 or n == len(self._all_items)
            else ui_text("selected_count", self._lang).format(n=n)
        )


# ------------------------------------------------------------------ Workers

class SearchWorker(QThread):
    results_ready = pyqtSignal(list)

    def __init__(self, keywords: list, mode: str, pos_filter: str,
                 artist_filter: list, cross_line: bool, jp_only: bool = False,
                 dedup: bool = True):
        super().__init__()
        self.keywords      = keywords
        self.mode          = mode
        self.pos_filter    = pos_filter
        self.artist_filter = artist_filter
        self.cross_line    = cross_line
        self.jp_only       = jp_only
        self.dedup         = dedup

    def run(self):
        field  = "surface" if self.mode == "surface" else "lemma"
        kw_ph  = ",".join(["?"] * len(self.keywords))
        params = list(self.keywords)

        pos_clause = ""
        if self.pos_filter:
            pos_clause = "AND t.pos = ?"
            params.append(self.pos_filter)
        artist_clause = ""
        if self.artist_filter:
            conds = " OR ".join(
                ["'/' || s.artist || '/' LIKE '%/' || ? || '/%'"] * len(self.artist_filter)
            )
            artist_clause = f"AND ({conds})"
            params.extend(self.artist_filter)

        if self.dedup:
            sql = f"""
                SELECT s.artist, s.title, MIN(u.time_sec), u.text,
                       MIN(t.token_idx), MIN(u.id), s.audio_path, u.song_id,
                       MIN(u.line_idx), COUNT(*) AS repeat_count
                FROM tokens t
                JOIN utterances u ON u.id = t.utterance_id
                JOIN songs s ON s.id = u.song_id
                WHERE t.{field} IN ({kw_ph}) {pos_clause} {artist_clause}
                GROUP BY s.id, u.text
                ORDER BY s.artist, s.title, MIN(u.time_sec)
            """
        else:
            sql = f"""
                SELECT s.artist, s.title, u.time_sec, u.text,
                       t.token_idx, u.id, s.audio_path, u.song_id, u.line_idx, 1
                FROM tokens t
                JOIN utterances u ON u.id = t.utterance_id
                JOIN songs s ON s.id = u.song_id
                WHERE t.{field} IN ({kw_ph}) {pos_clause} {artist_clause}
                ORDER BY s.artist, s.title, u.time_sec
            """
        conn = sqlite3.connect(str(DB_PATH))
        rows = conn.execute(sql, params).fetchall()

        results = []
        for artist, title, time_sec, text, token_idx, utt_id, audio_path, song_id, line_idx, repeat_count in rows:
            tokens = conn.execute(
                "SELECT surface FROM tokens WHERE utterance_id = ? ORDER BY token_idx",
                (utt_id,)
            ).fetchall()
            surfs = [t[0] for t in tokens]
            left  = "".join(surfs[:token_idx])
            match = surfs[token_idx] if token_idx < len(surfs) else self.keywords[0]
            right = "".join(surfs[token_idx + 1:])

            if self.cross_line:
                prev = conn.execute(
                    "SELECT text FROM utterances WHERE song_id=? AND line_idx=?",
                    (song_id, line_idx - 1)
                ).fetchone()
                nxt = conn.execute(
                    "SELECT text FROM utterances WHERE song_id=? AND line_idx=?",
                    (song_id, line_idx + 1)
                ).fetchone()
                if prev: left  = prev[0] + " / " + left
                if nxt:  right = right + " / " + nxt[0]

            results.append({
                "artist": artist, "title": title, "time_sec": time_sec,
                "text": text, "left": left, "match": match,
                "right": right, "audio_path": audio_path,
                "utterance_id": utt_id,
                "repeat_count": repeat_count,
            })

        conn.close()
        if self.jp_only:
            results = [r for r in results if _JP_RE.search(r["match"])]
        self.results_ready.emit(results)


class StatsWorker(QThread):
    results_ready = pyqtSignal(list)

    def __init__(self, pos_filter: str, artist_filter: list, jp_only: bool = False):
        super().__init__()
        self.pos_filter    = pos_filter
        self.artist_filter = artist_filter
        self.jp_only       = jp_only

    def run(self):
        pos_clause = "AND t.pos = ?" if self.pos_filter else ""
        params = []
        if self.pos_filter:
            params.append(self.pos_filter)
        artist_clause = ""
        if self.artist_filter:
            conds = " OR ".join(
                ["'/' || s.artist || '/' LIKE '%/' || ? || '/%'"] * len(self.artist_filter)
            )
            artist_clause = f"AND ({conds})"
            params.extend(self.artist_filter)
        sql = f"""
            SELECT t.lemma, t.pos,
                   COUNT(*)                          AS freq,
                   COUNT(DISTINCT u.song_id)         AS song_count,
                   GROUP_CONCAT(DISTINCT t.surface)  AS surfaces
            FROM tokens t
            JOIN utterances u ON u.id = t.utterance_id
            JOIN songs s ON s.id = u.song_id
            WHERE t.pos NOT IN ('PUNCT','SYM') {pos_clause} {artist_clause}
            GROUP BY t.lemma, t.pos
            ORDER BY freq DESC
            LIMIT 300
        """
        conn = sqlite3.connect(str(DB_PATH))
        rows = conn.execute(sql, params).fetchall()
        conn.close()
        if self.jp_only:
            rows = [r for r in rows if _JP_RE.search(r[0])]
        self.results_ready.emit(rows)


_POS_JA = {
    "NOUN": "名詞", "VERB": "動詞", "ADJ": "形容詞", "ADV": "副詞",
    "AUX": "助動詞", "PRON": "代名詞", "PROPN": "固有名詞",
    "INTJ": "感動詞", "ADP": "助詞", "CCONJ": "接続詞",
    "NUM": "数詞", "PART": "接辞", "SCONJ": "従属接", "DET": "限定詞",
}

# ------------------------------------------------------------------ Anki helpers

_ANKI_MODEL = "JPOP Corpus"


def _jisho_lookup(word: str) -> dict:
    """Jisho API で読み仮名 + 英語定義を取得。失敗時は空 dict。"""
    import urllib.request, urllib.parse, json
    url = "https://jisho.org/api/v1/search/words?keyword=" + urllib.parse.quote(word)
    try:
        with urllib.request.urlopen(url, timeout=8) as r:
            data = json.loads(r.read())
        items = data.get("data", [])
        if not items:
            return {}
        item = items[0]
        reading = ""
        if item.get("japanese"):
            j = item["japanese"][0]
            reading = j.get("reading") or j.get("word", "")
        defs = []
        for s in item.get("senses", [])[:3]:
            eng = "; ".join(s.get("english_definitions", [])[:4])
            pos = ", ".join(s.get("parts_of_speech", [])[:2])
            if eng:
                defs.append((pos, eng))
        return {"reading": reading, "defs": defs, "jlpt": item.get("jlpt", [])}
    except Exception:
        return {}


# ---- Local Yomitan Chinese dict helpers --------------------------------

def _sc_to_text(node) -> str:
    """Recursively extract plain text from a Yomitan structured-content node."""
    if isinstance(node, str):
        return node
    if isinstance(node, list):
        return "".join(_sc_to_text(c) for c in node)
    if isinstance(node, dict):
        return _sc_to_text(node.get("content", node.get("text", "")))
    return ""


def _sc_data(node) -> dict:
    return node.get("data", {}) if isinstance(node, dict) and isinstance(node.get("data"), dict) else {}


def _sc_name(node) -> str:
    data = _sc_data(node)
    return str(data.get("name") or data.get("orgtag") or "")


def _iter_sc_nodes(node):
    """Yield structured-content dict nodes in document order."""
    if isinstance(node, dict):
        yield node
        content = node.get("content", node.get("text", ""))
        yield from _iter_sc_nodes(content)
    elif isinstance(node, list):
        for child in node:
            yield from _iter_sc_nodes(child)


def _sc_text_excluding(node, exclude_names: set[str]) -> str:
    """Extract text while skipping whole subtrees such as examples inside senses."""
    if isinstance(node, str):
        return node
    if isinstance(node, list):
        return "".join(_sc_text_excluding(c, exclude_names) for c in node)
    if isinstance(node, dict):
        if _sc_name(node) in exclude_names:
            return ""
        if node.get("tag") == "rt":
            return ""
        return _sc_text_excluding(node.get("content", node.get("text", "")), exclude_names)
    return ""


def _sc_text_by_lang(node, want_zh: bool) -> str:
    """Extract zh/non-zh spans from bilingual structured examples."""
    if isinstance(node, str):
        return "" if want_zh else node
    if isinstance(node, list):
        return "".join(_sc_text_by_lang(c, want_zh) for c in node)
    if isinstance(node, dict):
        lang = node.get("lang")
        if lang == "zh":
            return _sc_to_text(node) if want_zh else ""
        return _sc_text_by_lang(node.get("content", node.get("text", "")), want_zh)
    return ""


def _clean_def_text(text: str) -> str:
    import re
    text = text.replace("\u3000", " ")
    text = re.sub(r"\s+", " ", text).strip()
    return text


def _format_structured_item(sense: str, examples: list[tuple[str, str]] | None = None) -> dict:
    """Return a compact HTML definition item for Anki, plus plain text for dedupe."""
    import html, re
    sense = _clean_def_text(sense)
    examples = examples or []
    note_match = re.match(r"^(\[[^\]]+\])(.*)$", sense)
    if note_match and not examples:
        label = html.escape(note_match.group(1))
        body = html.escape(note_match.group(2).strip())
        plain = sense
        return {
            "html": (
                "<div class='ym-note'>"
                f"<span class='ym-note-label'>{label}</span>"
                f"<span class='ym-note-body'>{body}</span>"
                "</div>"
            ),
            "text": _clean_def_text(plain),
        }
    m = re.match(r"^([①②③④⑤⑥⑦⑧⑨⑩⑪⑫⑬⑭⑮⑯⑰⑱⑲⑳]|[0-9０-９]+)(?:〔([^〕]+)〕)?(.*)$", sense)
    if m:
        num = html.escape(m.group(1))
        tag = html.escape((m.group(2) or "").strip())
        body = html.escape(m.group(3).strip())
        tag_html = f"<span class='ym-tag'>〔{tag}〕</span>" if tag else ""
        parts = [
            "<div class='ym-sense'>"
            f"<span class='ym-index'>{num}</span>{tag_html}"
            f"<span class='ym-gloss'>{body}</span>"
            "</div>"
        ]
    else:
        parts = [
            "<div class='ym-sense'>"
            f"<span class='ym-gloss'>{html.escape(sense)}</span>"
            "</div>"
        ]
    for ja, zh in examples[:3]:
        ja = _clean_def_text(ja).strip("「」")
        zh = _clean_def_text(zh)
        if zh:
            parts.append(
                "<div class='ym-example'>"
                f"<span class='ym-ja'>「{html.escape(ja)}」</span>"
                f"<span class='ym-zh'>{html.escape(zh)}</span>"
                "</div>"
            )
        elif ja:
            parts.append(
                "<div class='ym-example'>"
                f"<span class='ym-ja'>「{html.escape(ja)}」</span>"
                "</div>"
            )
    plain = sense + " " + " ".join(f"{a} {b}".strip() for a, b in examples[:3])
    return {"html": "".join(parts), "text": _clean_def_text(plain)}


def _extract_yomitan_structured_items(raw) -> list[dict]:
    """
    Convert Yomitan structured-content to small sense/example blocks.
    This keeps the useful Yomitan hierarchy instead of flattening a whole entry
    into one unreadable paragraph.
    """
    root = raw.get("content") if isinstance(raw, dict) and raw.get("type") == "structured-content" else raw
    items: list[dict] = []
    current: dict | None = None

    for node in _iter_sc_nodes(root):
        name = _sc_name(node)
        if name in {"meaning", "語義"}:
            sense = _clean_def_text(_sc_text_excluding(node, {"example", "用例G"}))
            if not sense:
                continue
            current = {"sense": sense, "examples": []}
            items.append(current)
        elif name in {"example", "用例G"}:
            all_text = _clean_def_text(_sc_to_text(node)).strip("「」")
            zh_text = _clean_def_text(_sc_text_by_lang(node, True))
            ja_text = _clean_def_text(_sc_text_by_lang(node, False)).strip("「」")
            if not ja_text and not zh_text:
                ja_text = all_text
            if current is None:
                current = {"sense": all_text, "examples": []}
                items.append(current)
            else:
                current["examples"].append((ja_text, zh_text))

    result = [
        _format_structured_item(it["sense"], it.get("examples", []))
        for it in items
        if it.get("sense")
    ]
    return result


def _parse_yomitan_term_defs(raw_defs: list) -> list:
    """
    Parse generic Yomitan term definitions for dict_terms.
    Returns a JSON-serialisable list; entries may be plain strings or
    {"html", "text"} objects for structured content.
    """
    defs: list = []
    for raw in raw_defs:
        if isinstance(raw, str):
            for line in raw.split("\n"):
                line = _clean_def_text(line)
                if line and len(line) > 1:
                    defs.append(line)
            continue
        structured = _extract_yomitan_structured_items(raw)
        if structured:
            defs.extend(structured)
            continue
        txt = _clean_def_text(_sc_to_text(raw))
        if txt and len(txt) > 1:
            defs.append(txt)
    return defs[:12]


def _split_flattened_def_text(text: str) -> list[str]:
    """Best-effort cleanup for old dict_terms rows imported before structured parsing."""
    import re
    text = _clean_def_text(text)
    if len(text) < 260:
        return [text] if text else []
    parts = re.split(r"(?=(?:[1-9][0-9]?〔|[①②③④⑤⑥⑦⑧⑨⑩⑪⑫⑬⑭⑮⑯⑰⑱⑲⑳]))", text)
    parts = [_clean_def_text(p) for p in parts if _clean_def_text(p)]
    if len(parts) <= 1:
        parts = re.split(r"(?=（[一二三四五六七八九十０-９0-9]+）)", text)
        parts = [_clean_def_text(p) for p in parts if _clean_def_text(p)]
    return parts[:10] if parts else ([text] if text else [])


def _coerce_definition_items(item, term_prefix: str = "") -> list:
    """Normalise plain/structured definition records to renderable items."""
    if isinstance(item, dict):
        html_body = item.get("html")
        if html_body:
            if term_prefix:
                import html
                html_body = (
                    f"<div class='ym-term-ref'>{html.escape(term_prefix)}</div>"
                    + html_body
                )
            return [{"html": html_body, "text": item.get("text", "")}]
        text = item.get("text", "")
        if text:
            return _coerce_definition_items(str(text), term_prefix)
        return []
    if not isinstance(item, str):
        item = _sc_to_text(item)
    parts = _split_flattened_def_text(item)
    if term_prefix and parts:
        parts = [term_prefix + parts[0]] + parts[1:]
    return parts


def _definition_dedupe_text(item) -> str:
    if isinstance(item, dict):
        return _clean_def_text(item.get("text") or item.get("html") or "")
    return _clean_def_text(str(item))


_DICT_LABEL = {'31': '明鏡', '43': '小学館'}


def _parse_yomitan_zh_defs(raw_defs: list, dict_key: str) -> list:
    """
    Extract Chinese definitions. Returns list of [source_label, def_text] pairs.
    source_label is the dictionary short name (e.g. '明鏡', '小学館').
    """
    import re
    _has_zh = lambda s: any('\u4e00' <= c <= '\u9fff' for c in s)
    label = _DICT_LABEL.get(dict_key, dict_key)

    strings = []
    for raw in raw_defs:
        if isinstance(raw, str):
            strings.append(raw)
        elif isinstance(raw, (dict, list)):
            txt = _sc_to_text(raw).strip()
            if txt:
                strings.append(txt)

    results = []
    for raw in strings:
        if dict_key == '31':
            for line in raw.split('\n')[1:]:
                line = line.strip()
                if line.startswith('▲') or line.startswith('ᐅ') or '/' not in line:
                    continue
                zh = line.split('/', 1)[1].strip()
                zh = re.sub(r'「[^「」]*」', '', zh).strip()
                if zh and _has_zh(zh):
                    results.append([label, zh])
        elif dict_key == '43':
            for line in raw.split('\n')[1:]:
                line = line.strip()
                if not line.startswith('（'):
                    continue
                line = re.sub(r'〔[^〔〕]*〕', '', line)
                line = re.sub(r'^（[０-９\d]+）\s*', '', line)
                line = re.sub(r'[a-zA-Zāáǎàēéěèīíǐìōóǒòūúǔùǖǘǚǜü]+', '', line)
                line = re.sub(r'[^\u4e00-\u9fff\uff00-\uffef，。；：！？、…～·\s]', '', line)
                line = line.strip().strip('，。').strip()
                if len(line) > 1 and _has_zh(line):
                    results.append([label, line])
        else:
            for line in raw.split('\n'):
                line = line.strip()
                if _has_zh(line) and len(line) > 1:
                    results.append([label, line])
    return results[:8]


def _ensure_yomitan_cache(zip_path: str) -> bool:
    """
    First-time setup: parse the outer Yomitan zip and build three tables in corpus.db:
      yomitan_zh    – Chinese definitions (明鏡双解 + 小学館)
      yomitan_pitch – pitch accent positions (アクセント辞典v2)
      yomitan_freq  – JPDB frequency rank
    Subsequent calls return immediately if tables are already populated.
    """
    import zipfile, io
    if not zip_path or not pathlib.Path(zip_path).exists():
        return False
    conn = sqlite3.connect(str(DB_PATH))
    try:
        conn.executescript("""
            CREATE TABLE IF NOT EXISTS yomitan_zh (
                term    TEXT PRIMARY KEY,
                reading TEXT NOT NULL DEFAULT '',
                zh_defs TEXT NOT NULL DEFAULT '[]'
            );
            CREATE TABLE IF NOT EXISTS yomitan_pitch (
                term    TEXT NOT NULL,
                reading TEXT NOT NULL DEFAULT '',
                positions TEXT NOT NULL DEFAULT '[]',
                PRIMARY KEY (term, reading)
            );
            CREATE TABLE IF NOT EXISTS yomitan_freq (
                term    TEXT PRIMARY KEY,
                freq    INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS yomitan_meta (
                key TEXT PRIMARY KEY,
                val TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_yomitan_zh_reading ON yomitan_zh(reading);
        """)
        zh_count    = conn.execute("SELECT COUNT(*) FROM yomitan_zh").fetchone()[0]
        pitch_count = conn.execute("SELECT COUNT(*) FROM yomitan_pitch").fetchone()[0]
        freq_count  = conn.execute("SELECT COUNT(*) FROM yomitan_freq").fetchone()[0]
        cache_ver   = conn.execute(
            "SELECT val FROM yomitan_meta WHERE key='zh_version'").fetchone()
        cache_ver   = int(cache_ver[0]) if cache_ver else 0

        # Version 3 = merged defs with [source, text] pairs
        if zh_count > 0 and cache_ver < 3:
            conn.execute("DELETE FROM yomitan_zh")
            conn.commit()
            zh_count = 0

        if zh_count > 0 and pitch_count > 0 and freq_count > 0:
            conn.close()
            return True

        with zipfile.ZipFile(zip_path) as oz:
            names = oz.namelist()

            # ── Chinese definitions ──────────────────────────────────────
            if zh_count == 0:
                # Dict 31 (明鏡双解): primary source, INSERT
                for dict_key in ['31']:
                    targets = [n for n in names if f'/{dict_key} ' in n and 'Bilingual' in n]
                    if not targets:
                        continue
                    with zipfile.ZipFile(io.BytesIO(oz.read(targets[0]))) as iz:
                        batch = []
                        for tb in sorted(n for n in iz.namelist() if n.startswith('term_bank_')):
                            for e in json.loads(iz.read(tb)):
                                zh_defs = _parse_yomitan_zh_defs(
                                    e[5] if len(e) > 5 else [], dict_key)
                                if zh_defs:
                                    batch.append((e[0], e[1] or '',
                                                  json.dumps(zh_defs, ensure_ascii=False)))
                        conn.executemany("INSERT OR IGNORE INTO yomitan_zh VALUES (?,?,?)", batch)
                        conn.commit()

                # Dict 43 (小学館): merge with existing entries, don't discard
                for dict_key in ['43']:
                    targets = [n for n in names if f'/{dict_key} ' in n and 'Bilingual' in n]
                    if not targets:
                        continue
                    with zipfile.ZipFile(io.BytesIO(oz.read(targets[0]))) as iz:
                        for tb in sorted(n for n in iz.namelist() if n.startswith('term_bank_')):
                            for e in json.loads(iz.read(tb)):
                                new_defs = _parse_yomitan_zh_defs(
                                    e[5] if len(e) > 5 else [], dict_key)
                                if not new_defs:
                                    continue
                                term, reading = e[0], e[1] or ''
                                row = conn.execute(
                                    "SELECT zh_defs FROM yomitan_zh WHERE term=?",
                                    (term,)).fetchone()
                                if row:
                                    old = json.loads(row[0])
                                    # Deduplicate by def_text (second element)
                                    seen = {p[1] for p in old if isinstance(p, list)}
                                    extra = [p for p in new_defs
                                             if isinstance(p, list) and p[1] not in seen]
                                    merged = (old + extra)[:12]
                                    conn.execute(
                                        "UPDATE yomitan_zh SET zh_defs=? WHERE term=?",
                                        (json.dumps(merged, ensure_ascii=False), term))
                                else:
                                    conn.execute(
                                        "INSERT INTO yomitan_zh VALUES (?,?,?)",
                                        (term, reading,
                                         json.dumps(new_defs, ensure_ascii=False)))
                        conn.commit()

                conn.execute("INSERT OR REPLACE INTO yomitan_meta VALUES ('zh_version','3')")
                conn.commit()

            # ── Pitch accent ─────────────────────────────────────────────
            if pitch_count == 0:
                targets = [n for n in names if '/21 ' in n]
                if targets:
                    with zipfile.ZipFile(io.BytesIO(oz.read(targets[0]))) as iz:
                        batch = []
                        for tb in sorted(n for n in iz.namelist()
                                         if n.startswith('term_meta_bank_')):
                            for e in json.loads(iz.read(tb)):
                                if e[1] != 'pitch':
                                    continue
                                meta = e[2]
                                reading = meta.get('reading', '')
                                positions = [p.get('position', 0)
                                             for p in meta.get('pitches', [])]
                                batch.append((e[0], reading,
                                              json.dumps(positions)))
                        conn.executemany(
                            "INSERT OR IGNORE INTO yomitan_pitch VALUES (?,?,?)", batch)
                        conn.commit()

            # ── JPDB frequency ───────────────────────────────────────────
            if freq_count == 0:
                targets = [n for n in names if '/11 ' in n]
                if targets:
                    with zipfile.ZipFile(io.BytesIO(oz.read(targets[0]))) as iz:
                        batch = []
                        for tb in sorted(n for n in iz.namelist()
                                         if n.startswith('term_meta_bank_')):
                            for e in json.loads(iz.read(tb)):
                                if e[1] != 'freq':
                                    continue
                                meta = e[2]
                                # Two formats: direct {value,displayValue} or nested {frequency:{…}}
                                if 'frequency' in meta:
                                    inner = meta['frequency']
                                    display = inner.get('displayValue', '')
                                    val = inner.get('value', 0)
                                else:
                                    display = meta.get('displayValue', '')
                                    val = meta.get('value', 0)
                                if '㋕' in display:  # kana-only entry, skip
                                    continue
                                batch.append((e[0], val))
                        conn.executemany(
                            "INSERT OR IGNORE INTO yomitan_freq VALUES (?,?)", batch)
                        conn.commit()

        # ── Pre-populate jlpt_cache from JPDB freq for all corpus lemmas ──
        # JPDB rank thresholds derived from correlation with JMdict JLPT markers:
        #   rank ≤  3000 → N5  (most common)
        #   rank ≤  8000 → N4
        #   rank ≤ 15000 → N3
        #   rank ≤ 25000 → N2
        #   rank ≤ 40000 → N1
        #   rank  > 40000 → ""  (beyond JLPT)
        _freq_to_jlpt = lambda r: (
            "N5" if r <=  3000 else
            "N4" if r <=  8000 else
            "N3" if r <= 15000 else
            "N2" if r <= 25000 else
            "N1" if r <= 40000 else ""
        )
        conn2 = sqlite3.connect(str(DB_PATH))
        try:
            conn2.execute(
                "CREATE TABLE IF NOT EXISTS jlpt_cache "
                "(lemma TEXT PRIMARY KEY, level TEXT NOT NULL DEFAULT '')"
            )
            # Fix existing "JLPT-N3" → "N3" in cache
            conn2.execute("""
                UPDATE jlpt_cache
                SET level = REPLACE(level, 'JLPT-', '')
                WHERE level LIKE 'JLPT-%'
            """)
            lemma_rows = conn2.execute(
                "SELECT DISTINCT lemma FROM tokens"
            ).fetchall()
            corpus_lemmas = [r[0] for r in lemma_rows]
            if corpus_lemmas:
                ph = ",".join("?" * len(corpus_lemmas))
                freq_map = dict(conn2.execute(
                    f"SELECT term, freq FROM yomitan_freq WHERE term IN ({ph})",
                    corpus_lemmas
                ).fetchall())
                already = set(r[0] for r in conn2.execute(
                    f"SELECT lemma FROM jlpt_cache WHERE lemma IN ({ph})",
                    corpus_lemmas
                ).fetchall())
                batch = [
                    (lm, _freq_to_jlpt(freq_map[lm]))
                    for lm in corpus_lemmas
                    if lm not in already and lm in freq_map
                ]
                if batch:
                    conn2.executemany(
                        "INSERT OR IGNORE INTO jlpt_cache VALUES (?,?)", batch)
            conn2.commit()
        except Exception:
            pass
        finally:
            conn2.close()

        conn.close()
        _register_legacy_tables()
        return True
    except Exception:
        conn.close()
        return False


_yomitan_ready: bool | None = None  # None=unchecked; kept for bg-worker compat


def _has_local_dicts() -> bool:
    """Return True if at least one enabled term dict exists (local lookup available)."""
    try:
        conn = sqlite3.connect(str(DB_PATH))
        n = conn.execute(
            "SELECT COUNT(*) FROM dict_registry WHERE enabled=1 AND dict_type='terms'"
        ).fetchone()[0]
        conn.close()
        return n > 0
    except Exception:
        return False

# Special zip_path markers for legacy tables registered in dict_registry
_LEGACY_MEIKYO_KEY      = "#legacy_terms_明鏡"
_LEGACY_SHOGAKUKAN_KEY  = "#legacy_terms_小学館"
_LEGACY_PITCH_KEY       = "#legacy_pitch"
_LEGACY_FREQ_KEY        = "#legacy_freq"
_ALL_LEGACY_TERM_KEYS   = (_LEGACY_MEIKYO_KEY, _LEGACY_SHOGAKUKAN_KEY)


def _check_yomitan_table(table: str) -> bool:
    try:
        conn = sqlite3.connect(str(DB_PATH))
        count = conn.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]
        conn.close()
        return count > 0
    except Exception:
        return False


def _ensure_dict_tables():
    """Create/migrate dict_registry and dict_terms tables."""
    conn = sqlite3.connect(str(DB_PATH))
    try:
        conn.executescript("""
            CREATE TABLE IF NOT EXISTS dict_registry (
                name        TEXT PRIMARY KEY,
                zip_path    TEXT NOT NULL DEFAULT '',
                dict_type   TEXT NOT NULL DEFAULT 'terms',
                entry_count INTEGER NOT NULL DEFAULT 0,
                enabled     INTEGER NOT NULL DEFAULT 1,
                sort_order  INTEGER NOT NULL DEFAULT 999,
                revision    TEXT NOT NULL DEFAULT '',
                imported_at TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS dict_terms (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                term      TEXT NOT NULL,
                reading   TEXT NOT NULL DEFAULT '',
                dict_name TEXT NOT NULL,
                defs_json TEXT NOT NULL DEFAULT '[]'
            );
            CREATE INDEX IF NOT EXISTS idx_dt_term_dict ON dict_terms(term, dict_name);
            CREATE INDEX IF NOT EXISTS idx_dt_reading ON dict_terms(reading);
        """)
        # Migrate existing tables: add new columns if absent
        cols = {r[1] for r in conn.execute("PRAGMA table_info(dict_registry)").fetchall()}
        if 'sort_order' not in cols:
            conn.execute("ALTER TABLE dict_registry ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 999")
        if 'revision' not in cols:
            conn.execute("ALTER TABLE dict_registry ADD COLUMN revision TEXT NOT NULL DEFAULT ''")
        dt_info = conn.execute("PRAGMA table_info(dict_terms)").fetchall()
        dt_cols = {r[1] for r in dt_info}
        # Older versions used PRIMARY KEY(term, dict_name), which silently
        # discarded Yomitan homograph entries. Rebuild as row-id table.
        if "id" not in dt_cols:
            conn.execute("ALTER TABLE dict_terms RENAME TO dict_terms_old")
            conn.executescript("""
                CREATE TABLE dict_terms (
                    id        INTEGER PRIMARY KEY AUTOINCREMENT,
                    term      TEXT NOT NULL,
                    reading   TEXT NOT NULL DEFAULT '',
                    dict_name TEXT NOT NULL,
                    defs_json TEXT NOT NULL DEFAULT '[]'
                );
                INSERT INTO dict_terms(term, reading, dict_name, defs_json)
                SELECT term, reading, dict_name, defs_json FROM dict_terms_old;
                DROP TABLE dict_terms_old;
                CREATE INDEX IF NOT EXISTS idx_dt_term_dict ON dict_terms(term, dict_name);
                CREATE INDEX IF NOT EXISTS idx_dt_reading ON dict_terms(reading);
            """)
        conn.commit()
    finally:
        conn.close()


def _register_legacy_tables():
    """Register pre-existing yomitan_zh/pitch/freq tables in dict_registry so they
    appear in DictManagerDialog alongside user-imported dicts.
    明鏡 and 小学館 are registered as separate entries."""
    from datetime import datetime as _dt
    conn = sqlite3.connect(str(DB_PATH))
    try:
        # Migrate old merged "明鏡/小学館（旧缓存）" entry → remove it if present
        conn.execute("DELETE FROM dict_registry WHERE zip_path='#legacy_terms'")

        total_zh = 0
        try:
            total_zh = conn.execute("SELECT COUNT(*) FROM yomitan_zh").fetchone()[0]
        except Exception:
            pass

        if total_zh > 0:
            # Count per source via LIKE (fast approximation)
            for src_label, key in [("明鏡", _LEGACY_MEIKYO_KEY),
                                    ("小学館", _LEGACY_SHOGAKUKAN_KEY)]:
                try:
                    cnt = conn.execute(
                        "SELECT COUNT(*) FROM yomitan_zh WHERE zh_defs LIKE ?",
                        (f'%"{src_label}"%',)).fetchone()[0]
                except Exception:
                    cnt = 0
                if cnt == 0:
                    continue
                existing = conn.execute(
                    "SELECT name FROM dict_registry WHERE zip_path=?", (key,)).fetchone()
                if existing:
                    conn.execute("UPDATE dict_registry SET entry_count=? WHERE zip_path=?",
                                 (cnt, key))
                else:
                    so = (conn.execute(
                        "SELECT MAX(sort_order) FROM dict_registry").fetchone()[0] or 0) + 1
                    conn.execute(
                        "INSERT INTO dict_registry VALUES (?,?,?,?,?,?,?,?)",
                        (src_label, key, "terms", cnt, 1, so, "旧缓存",
                         _dt.now().strftime('%Y-%m-%d')))

        # Pitch and freq — single entries
        for table, key, dtype, name in [
            ("yomitan_pitch", _LEGACY_PITCH_KEY, "pitch", "アクセント辞典（旧缓存）"),
            ("yomitan_freq",  _LEGACY_FREQ_KEY,  "freq",  "JPDB 词频（旧缓存）"),
        ]:
            try:
                cnt = conn.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]
            except Exception:
                cnt = 0
            if cnt == 0:
                continue
            existing = conn.execute(
                "SELECT name FROM dict_registry WHERE zip_path=?", (key,)).fetchone()
            if existing:
                conn.execute("UPDATE dict_registry SET entry_count=? WHERE zip_path=?",
                             (cnt, key))
            else:
                so = (conn.execute(
                    "SELECT MAX(sort_order) FROM dict_registry").fetchone()[0] or 0) + 1
                conn.execute(
                    "INSERT INTO dict_registry VALUES (?,?,?,?,?,?,?,?)",
                    (name, key, dtype, cnt, 1, so, "旧缓存",
                     _dt.now().strftime('%Y-%m-%d')))
        conn.commit()
    finally:
        conn.close()


def _delete_legacy_terms_source(conn, src_label: str):
    """Remove all defs with the given source label from yomitan_zh in-place."""
    rows = conn.execute("SELECT term, zh_defs FROM yomitan_zh").fetchall()
    to_update, to_delete = [], []
    for term, raw in rows:
        try:
            defs = json.loads(raw)
        except Exception:
            continue
        filtered = [d for d in defs
                    if not (isinstance(d, list) and len(d) >= 2 and d[0] == src_label)]
        if not filtered:
            to_delete.append((term,))
        elif len(filtered) < len(defs):
            to_update.append((json.dumps(filtered, ensure_ascii=False), term))
    if to_update:
        conn.executemany("UPDATE yomitan_zh SET zh_defs=? WHERE term=?", to_update)
    if to_delete:
        conn.executemany("DELETE FROM yomitan_zh WHERE term=?", to_delete)


def _detect_yomitan_dict_type(z) -> str:
    """Return 'terms', 'freq', or 'pitch' from a ZipFile object."""
    names = z.namelist()
    if any(n.startswith('term_bank_') for n in names):
        return 'terms'
    for mb in sorted(n for n in names if n.startswith('term_meta_bank_')):
        try:
            entries = json.loads(z.read(mb))
            if entries and len(entries[0]) > 1:
                kind = entries[0][1]
                if kind == 'pitch':
                    return 'pitch'
                if kind == 'freq':
                    return 'freq'
        except Exception:
            pass
        break
    return 'terms'


def _import_single_yomitan_dict(zip_path: str, conn, progress_cb=None) -> tuple:
    """
    Import any Yomitan-format zip. Auto-detects type (terms/freq/pitch).
    Terms → dict_terms table; Freq → yomitan_freq; Pitch → yomitan_pitch.
    Returns (dict_name, entry_count, dict_type).
    """
    import zipfile
    from datetime import datetime as _dt
    with zipfile.ZipFile(zip_path) as z:
        names = z.namelist()
        if 'index.json' not in names:
            raise ValueError("缺少 index.json，不是有效的 Yomitan 词典格式")
        meta      = json.loads(z.read('index.json'))
        dict_name = (meta.get('title') or pathlib.Path(zip_path).stem).strip()
        revision  = str(meta.get('revision', ''))
        if not dict_name:
            raise ValueError("index.json 中没有 title 字段")

        dict_type = _detect_yomitan_dict_type(z)

        # Preserve sort_order on re-import; append new dicts at end
        existing  = conn.execute(
            "SELECT sort_order FROM dict_registry WHERE name=?", (dict_name,)).fetchone()
        if existing:
            sort_order = existing[0]
        else:
            row = conn.execute("SELECT MAX(sort_order) FROM dict_registry").fetchone()
            sort_order = (row[0] or 0) + 1

        conn.execute("DELETE FROM dict_registry WHERE name=?", (dict_name,))
        conn.commit()

        count = 0

        if dict_type == 'terms':
            conn.execute("DELETE FROM dict_terms WHERE dict_name=?", (dict_name,))
            conn.commit()
            term_banks = sorted(n for n in names if n.startswith('term_bank_'))
            total = len(term_banks)
            batch = []
            for idx, tb in enumerate(term_banks):
                for e in json.loads(z.read(tb)):
                    term    = (e[0] if len(e) > 0 else '') or ''
                    reading = (e[1] if len(e) > 1 else '') or ''
                    if not term:
                        continue
                    defs = _parse_yomitan_term_defs(e[5] if len(e) > 5 else [])
                    if not defs:
                        continue
                    batch.append((term, reading, dict_name,
                                   json.dumps(defs, ensure_ascii=False)))
                if len(batch) >= 5000:
                    conn.executemany(
                        "INSERT INTO dict_terms(term, reading, dict_name, defs_json) "
                        "VALUES (?,?,?,?)", batch)
                    count += len(batch)
                    batch = []
                if progress_cb:
                    progress_cb(idx + 1, total)
            if batch:
                conn.executemany(
                    "INSERT INTO dict_terms(term, reading, dict_name, defs_json) "
                    "VALUES (?,?,?,?)", batch)
                count += len(batch)
            conn.commit()

        elif dict_type == 'freq':
            meta_banks = sorted(n for n in names if n.startswith('term_meta_bank_'))
            total = len(meta_banks)
            batch = []
            for idx, mb in enumerate(meta_banks):
                for e in json.loads(z.read(mb)):
                    if len(e) < 3 or e[1] != 'freq':
                        continue
                    m = e[2]
                    if 'frequency' in m:
                        inner = m['frequency']
                        val, disp = inner.get('value', 0), inner.get('displayValue', '')
                    else:
                        val, disp = m.get('value', 0), m.get('displayValue', '')
                    if '㋕' in disp:
                        continue
                    batch.append((e[0], val))
                if len(batch) >= 5000:
                    conn.executemany("INSERT OR REPLACE INTO yomitan_freq VALUES (?,?)", batch)
                    count += len(batch)
                    batch = []
                if progress_cb:
                    progress_cb(idx + 1, total)
            if batch:
                conn.executemany("INSERT OR REPLACE INTO yomitan_freq VALUES (?,?)", batch)
                count += len(batch)
            conn.commit()

        elif dict_type == 'pitch':
            meta_banks = sorted(n for n in names if n.startswith('term_meta_bank_'))
            total = len(meta_banks)
            batch = []
            for idx, mb in enumerate(meta_banks):
                for e in json.loads(z.read(mb)):
                    if len(e) < 3 or e[1] != 'pitch':
                        continue
                    m = e[2]
                    positions = [p.get('position', 0) for p in m.get('pitches', [])]
                    batch.append((e[0], m.get('reading', ''), json.dumps(positions)))
                if len(batch) >= 5000:
                    conn.executemany("INSERT OR IGNORE INTO yomitan_pitch VALUES (?,?,?)", batch)
                    count += len(batch)
                    batch = []
                if progress_cb:
                    progress_cb(idx + 1, total)
            if batch:
                conn.executemany("INSERT OR IGNORE INTO yomitan_pitch VALUES (?,?,?)", batch)
                count += len(batch)
            conn.commit()

    conn.execute(
        "INSERT OR REPLACE INTO dict_registry VALUES (?,?,?,?,?,?,?,?)",
        (dict_name, str(zip_path), dict_type, count, 1,
         sort_order, revision, _dt.now().strftime('%Y-%m-%d')))
    conn.commit()
    return dict_name, count, dict_type


def _lookup_yomitan_zh(word: str) -> dict:
    """
    Unified dict lookup via dict_registry (sort_order, enabled).
    明鏡 / 小学館 legacy entries filter yomitan_zh by source label.
    """
    reading = ""
    defs: list = []
    seen: set = set()

    try:
        conn = sqlite3.connect(str(DB_PATH))
        enabled = conn.execute(
            "SELECT name, zip_path FROM dict_registry "
            "WHERE enabled=1 AND dict_type='terms' ORDER BY sort_order ASC"
        ).fetchall()

        def _term_rows(table_sql: str, params_exact: tuple, params_reading: tuple):
            rows = conn.execute(table_sql + " AND term=?", params_exact).fetchall()
            if rows:
                return rows
            return conn.execute(table_sql + " AND reading=?", params_reading).fetchall()

        max_defs_per_dict = 8
        for dict_name, zip_path in enabled:
            if zip_path in _ALL_LEGACY_TERM_KEYS:
                # Filter yomitan_zh by this source label (明鏡 or 小学館)
                src_filter = ("明鏡" if zip_path == _LEGACY_MEIKYO_KEY else "小学館")
                rows = _term_rows(
                    "SELECT term, reading, zh_defs FROM yomitan_zh WHERE 1=1",
                    (word,), (word,)
                )
                added_for_dict = 0
                for term, rd, zh_defs_json in rows[:8]:
                    if not reading and rd:
                        reading = rd
                    pfx = f"【{term}】" if term != word else ""
                    for sense in json.loads(zh_defs_json):
                        if not (isinstance(sense, list) and len(sense) >= 2
                                and sense[0] == src_filter):
                            continue
                        src, txt = sense[0], sense[1]
                        full = pfx + txt if pfx else txt
                        k = (src, full)
                        if k not in seen:
                            seen.add(k)
                            defs.append((src, full))
                            added_for_dict += 1
                            if added_for_dict >= max_defs_per_dict:
                                break
                    if added_for_dict >= max_defs_per_dict:
                        break
            else:
                # Query dict_terms
                exact = conn.execute(
                    "SELECT term, reading, dict_name, defs_json FROM dict_terms "
                    "WHERE dict_name=? AND term=?",
                    (dict_name, word)
                ).fetchall()
                rows = exact or conn.execute(
                    "SELECT term, reading, dict_name, defs_json FROM dict_terms "
                    "WHERE dict_name=? AND reading=?",
                    (dict_name, word)
                ).fetchall()
                added_for_dict = 0
                for term, rd, _dn, defs_json in rows[:8]:
                    if not reading and rd:
                        reading = rd
                    pfx = f"【{term}】" if term != word else ""
                    for item in json.loads(defs_json):
                        for full in _coerce_definition_items(item, pfx):
                            k = (dict_name, _definition_dedupe_text(full))
                            if k in seen:
                                continue
                            seen.add(k)
                            defs.append((dict_name, full))
                            added_for_dict += 1
                            if added_for_dict >= max_defs_per_dict:
                                break
                        if added_for_dict >= max_defs_per_dict:
                            break
                    if added_for_dict >= max_defs_per_dict:
                        break
        conn.close()
    except Exception:
        pass

    return {"reading": reading, "defs": defs, "jlpt": []}


def _lookup_yomitan_pitch(word: str, reading: str = "") -> str:
    """Return pitch accent string like 'こきゅう[0]', or '' if not found."""
    if not _check_yomitan_table("yomitan_pitch"):
        return ""
    try:
        conn = sqlite3.connect(str(DB_PATH))
        if reading:
            row = conn.execute(
                "SELECT reading, positions FROM yomitan_pitch WHERE term=? AND reading=?",
                (word, reading)
            ).fetchone()
        if not reading or not row:
            row = conn.execute(
                "SELECT reading, positions FROM yomitan_pitch WHERE term=? LIMIT 1",
                (word,)
            ).fetchone()
        conn.close()
    except Exception:
        return ""
    if not row:
        return ""
    rd, pos_json = row
    positions = json.loads(pos_json)
    if not positions:
        return ""
    pos_str = "".join(f"[{p}]" for p in positions[:3])
    return f"{rd}{pos_str}"


def _lookup_yomitan_freq(word: str) -> str:
    """Return JPDB frequency rank as string, or '' if not found."""
    if not _check_yomitan_table("yomitan_freq"):
        return ""
    try:
        conn = sqlite3.connect(str(DB_PATH))
        row = conn.execute(
            "SELECT freq FROM yomitan_freq WHERE term=?", (word,)
        ).fetchone()
        conn.close()
        return str(row[0]) if row and row[0] else ""
    except Exception:
        return ""


# ---- Translation fallback ----------------------------------------------

def _translate_en_to_zh(text: str) -> str:
    """Unofficial Google Translate endpoint: English → Simplified Chinese."""
    import urllib.request, urllib.parse, json
    url = ("https://translate.googleapis.com/translate_a/single"
           "?client=gtx&sl=en&tl=zh-CN&dt=t&q=" + urllib.parse.quote(text))
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=6) as r:
            data = json.loads(r.read())
        parts = [seg[0] for seg in data[0] if seg[0]]
        return "".join(parts)
    except Exception:
        return text


def _jotoba_lookup_zh(word: str) -> dict:
    """
    中文定義取得優先順位：
    1. Local Yomitan cache (明鏡双解/小学館, instant)
    2. Jotoba API
    3. Google Translate fallback (Jisho EN → ZH)
    """
    # 1. Local cache
    local = _lookup_yomitan_zh(word)
    if local.get("defs"):
        return local

    import urllib.request, json
    url = "https://jotoba.de/api/search/words"
    body = json.dumps({"query": word, "language": "Chinese", "no_english": False}).encode()
    reading = ""
    defs = []
    try:
        req = urllib.request.Request(
            url, data=body,
            headers={"Content-Type": "application/json",
                     "Accept": "application/json"})
        with urllib.request.urlopen(req, timeout=5) as r:
            data = json.loads(r.read())
        words = data.get("words", [])
        if words:
            w = words[0]
            rd = w.get("reading", {})
            reading = rd.get("kana") or rd.get("kanji", "")
            for s in w.get("senses", [])[:3]:
                glosses = s.get("glosses", [])
                pos_raw = s.get("pos", [])
                pos_str = ""
                if pos_raw:
                    first = pos_raw[0]
                    pos_str = first if isinstance(first, str) \
                              else next(iter(first.keys()), "")
                meaning = "；".join(str(g) for g in glosses[:4])
                # Skip if glosses look like English (Jotoba returned wrong language)
                if meaning and any('\u4e00' <= c <= '\u9fff' for c in meaning):
                    defs.append((pos_str, meaning))
    except Exception:
        pass

    if defs:
        return {"reading": reading, "defs": defs, "jlpt": []}

    # Fallback: Jisho English → Google Translate to Chinese
    en = _jisho_lookup(word)
    if not reading:
        reading = en.get("reading", "")
    zh_defs = []
    for pos, eng in en.get("defs", []):
        zh = _translate_en_to_zh(eng)
        zh_defs.append((pos, zh))
    return {
        "reading":  reading,
        "defs":     zh_defs if zh_defs else [(p, e) for p, e in en.get("defs", [])],
        "jlpt":     en.get("jlpt", []),
    }


def _lookup_candidates(lemma: str, pos: str = "", surface: str = "") -> list[str]:
    """
    Generate conservative dictionary lookup candidates for inflected/derived words.

    Expression stays unchanged; these candidates are only used to find definitions.
    """
    candidates: list[str] = []

    def add(value: str):
        value = _clean_def_text(value or "")
        if value and value not in candidates:
            candidates.append(value)

    lemma = lemma or ""
    surface = surface or ""
    pos = pos or ""
    add(lemma)
    add(surface)

    bases = [x for x in (lemma, surface) if x]

    if pos == "VERB":
        for base in bases:
            if base.endswith("する"):
                continue
            if base.endswith("し") and len(base) > 1:
                add(base[:-1] + "する")
            if re.search(r"[\u4e00-\u9fff]", base):
                add(base + "する")
        # Common irregular variants.
        if lemma in {"為る", "します", "して"}:
            add("する")
        if lemma in {"来る", "きた", "来た"}:
            add("来る")

    if pos != "VERB":
        for base in bases:
            if base.endswith("し") and len(base) > 2 and re.search(r"[\u4e00-\u9fff]", base):
                add(base[:-1] + "する")

    if pos in {"ADJ", "ADV"}:
        for base in bases:
            if base.endswith("く") and len(base) > 1:
                add(base[:-1] + "い")
            if base.endswith("かった") and len(base) > 3:
                add(base[:-3] + "い")
            if base.endswith("くない") and len(base) > 3:
                add(base[:-3] + "い")
            if base.endswith("に") and len(base) > 1:
                add(base[:-1])
        if (lemma in {"いい", "よい", "よかった", "よく"} or
                surface in {"いい", "よい", "よく", "よかった"}):
            add("良い")
            add("いい")

    if pos not in {"ADJ", "ADV"}:
        for base in bases:
            if base.endswith("く") and len(base) > 1:
                add(base[:-1] + "い")
            if base.endswith("かった") and len(base) > 3:
                add(base[:-3] + "い")

    for base in list(candidates):
        if base.endswith("し") and len(base) > 2 and re.search(r"[\u4e00-\u9fff]", base):
            suru = base[:-1] + "する"
            if suru in candidates:
                candidates.remove(suru)
                idx = candidates.index(base)
                candidates.insert(idx, suru)

    return candidates[:10]


def _common_surface_for_lemma(lemma: str, pos: str = "") -> str:
    try:
        conn = sqlite3.connect(str(DB_PATH))
        if pos:
            row = conn.execute(
                """
                SELECT surface
                FROM tokens
                WHERE lemma=? AND pos=? AND surface IS NOT NULL AND surface <> ''
                GROUP BY surface
                ORDER BY COUNT(*) DESC
                LIMIT 1
                """,
                (lemma, pos),
            ).fetchone()
        else:
            row = conn.execute(
                """
                SELECT surface
                FROM tokens
                WHERE lemma=? AND surface IS NOT NULL AND surface <> ''
                GROUP BY surface
                ORDER BY COUNT(*) DESC
                LIMIT 1
                """,
                (lemma,),
            ).fetchone()
        conn.close()
        return row[0] if row else ""
    except Exception:
        return ""


def _lookup_word_for_anki(
    lemma: str,
    pos: str = "",
    def_lang: str = "中文",
    surface: str = "",
    do_def: bool = True,
) -> dict:
    """
    Lookup definitions with fallback candidates, but keep card Expression unchanged.
    """
    if not surface:
        surface = _common_surface_for_lemma(lemma, pos)
    candidates = _lookup_candidates(lemma, pos, surface)
    lookup_fn = _jotoba_lookup_zh if def_lang == "中文" else _jisho_lookup

    if not do_def:
        result = {"reading": "", "defs": [], "jlpt": []}
        result["lookup_term"] = lemma
        result["lookup_candidates"] = candidates
        return result

    first_result = None
    if def_lang == "中文":
        for term in candidates or [lemma]:
            result = _lookup_yomitan_zh(term)
            if result.get("defs") or result.get("reading"):
                result["lookup_term"] = term
                result["lookup_candidates"] = candidates
                return result

    for term in candidates or [lemma]:
        result = lookup_fn(term)
        if first_result is None:
            first_result = result
        if result.get("defs") or result.get("reading") or result.get("jlpt"):
            result["lookup_term"] = term
            result["lookup_candidates"] = candidates
            return result

    result = first_result or {"reading": "", "defs": [], "jlpt": []}
    result["lookup_term"] = lemma
    result["lookup_candidates"] = candidates
    return result


def _prefix_lookup_term_html(meaning_html: str, expression: str, lookup_term: str) -> str:
    if not lookup_term or lookup_term == expression:
        return meaning_html
    import html as _html
    note = (
        "<div class='ym-term-ref'>"
        f"辞書形：{_html.escape(lookup_term)}"
        "</div>"
    )
    if meaning_html.startswith("<div class='defs'>"):
        return meaning_html.replace("<div class='defs'>", "<div class='defs'>" + note, 1)
    return note + meaning_html


def _clip_audio(audio_path: str, start_sec: float, end_sec: float, out_path: str) -> bool:
    """ffmpeg で音声をクリップ。ffmpeg がなければ False を返す。"""
    import shutil, subprocess
    ffmpeg = (shutil.which("ffmpeg")
              or (str(app_dir() / "ffmpeg.exe")
                  if (app_dir() / "ffmpeg.exe").exists() else None))
    if not ffmpeg or not pathlib.Path(audio_path).exists():
        return False
    end = end_sec if (end_sec and end_sec > start_sec + 0.5) else start_sec + 6.0
    cmd = [
        ffmpeg, "-y", "-loglevel", "error",
        "-i", audio_path,
        "-ss", f"{max(0.0, start_sec - 0.3):.3f}",
        "-to", f"{end + 0.5:.3f}",
        "-acodec", "libmp3lame", "-q:a", "5",
        out_path,
    ]
    try:
        r = subprocess.run(cmd, capture_output=True, timeout=30)
        return r.returncode == 0 and pathlib.Path(out_path).exists()
    except Exception:
        return False


_ANKI_CSS = """\
.card { font-family:"Meiryo","Yu Gothic UI",sans-serif; font-size:18px;
        text-align:center; padding:16px; }
.word { font-size:2.4em; font-weight:bold; margin:14px 0 4px; }
.reading { font-size:1.1em; opacity:.7; margin-bottom:4px; }
.badges { display:flex; justify-content:center; gap:6px; margin:4px 0 8px; flex-wrap:wrap; }
.jlpt  { display:inline-block; padding:2px 10px; border-radius:4px;
         font-size:.82em; font-weight:bold; color:#fff; background:#3498db; }
.pitch { display:inline-block; padding:2px 10px; border-radius:4px;
         font-size:.82em; font-weight:bold; color:#fff; background:#27ae60; }
.freq  { display:inline-block; padding:2px 10px; border-radius:4px;
         font-size:.82em; color:#fff; background:#8e44ad; }
hr { border:none; border-top:1px solid rgba(128,128,128,.35); margin:14px 0; }
.defs { text-align:left; max-width:980px; margin:0 auto; }
.dict-group { margin:10px 0 16px; padding:10px 12px 12px;
              background:rgba(127,127,127,.07); border-radius:8px; }
.dict-hdr { display:inline-block; font-size:.75em; font-weight:bold; color:#fff;
            padding:3px 12px; border-radius:999px; margin-bottom:8px;
            letter-spacing:0; }
.dict-hdr-明鏡   { background:#c0392b; }
.dict-hdr-小学館 { background:#2471a3; }
.dict-hdr-other  { background:#7f8c8d; }
.meaning { margin:0; }
.ym-item { margin:8px 0 10px; line-height:1.55; }
.ym-sense { display:block; }
.ym-index { display:inline-block; min-width:1.45em; margin-right:.35em;
            color:#dfe7ef; font-weight:700; }
.ym-num { display:inline-block; min-width:1.45em; margin-right:.35em;
          color:#dfe7ef; font-weight:700; }
.ym-tag { color:#d36b16; font-weight:700; margin-right:.35em; }
.ym-gloss { color:#f0f2f4; }
.ym-note { display:block; color:#d2d6dc; font-size:.92em; line-height:1.55; }
.ym-note-label { color:#f0c35b; font-weight:700; margin-right:.35em; }
.ym-note-body { opacity:.9; }
.ym-example { margin:.18em 0 .18em 1.75em; padding-left:.7em;
              border-left:2px solid rgba(150,160,170,.24); font-size:.94em; }
.ym-ja { color:#1f9d32; margin-right:.75em; }
.ym-zh { color:#3f8fd2; }
.ym-term-ref { color:#9aa7b2; font-size:.9em; margin:.1em 0 .2em; }
.sent { text-align:left; margin:6px 0; padding:8px 14px;
        background:rgba(192,57,43,.08); border-left:3px solid #c0392b;
        border-radius:0 4px 4px 0; font-size:.95em; }
.sent-src { font-size:.75em; opacity:.6; margin-left:8px; }
.src { font-size:.75em; opacity:.55; margin-top:10px; }
.pos-badge { display:inline-block; padding:2px 10px; border-radius:4px;
             font-size:.82em; color:#fff; background:#e67e22; }
"""

_ANKI_TEMPLATES = [{
    "Name": "JPOP Corpus Card",
    "Front": (
        '<div class="word">{{Expression}}</div>'
        '{{Sentence}}'
    ),
    "Back": (
        # ── Word info ──
        '<div class="word">{{Expression}}</div>'
        '<div class="reading">{{Reading}}</div>'
        '<div class="badges">'
        '{{#PartOfSpeech}}<span class="pos-badge">{{PartOfSpeech}}</span>{{/PartOfSpeech}}'
        '{{#JLPT}}<span class="jlpt">{{JLPT}}</span>{{/JLPT}}'
        '{{#Pitch}}<span class="pitch">{{Pitch}}</span>{{/Pitch}}'
        '{{#Freq}}<span class="freq">JPDB {{Freq}}</span>{{/Freq}}'
        '</div>'
        '<hr>'
        # ── Sentence ──
        '{{Sentence}}'
        '{{SentenceAudio}}'
        '{{#Source}}<div class="src">🎵 {{Source}}</div>{{/Source}}'
        '<hr>'
        # ── Definitions ──
        '{{#Meaning}}{{Meaning}}{{/Meaning}}'
    ),
}]


def _ensure_anki_model():
    """JPOP Corpus ノートタイプを作成または CSS を最新化する。"""
    existing = _anki_request("modelNames") or []
    fields = ["Expression", "Reading", "Meaning",
              "Sentence", "SentenceAudio", "Source", "JLPT", "Pitch", "Freq", "PartOfSpeech"]
    if _ANKI_MODEL not in existing:
        _anki_request("createModel",
            modelName=_ANKI_MODEL,
            inOrderFields=fields,
            css=_ANKI_CSS,
            cardTemplates=_ANKI_TEMPLATES,
        )
    else:
        # Add new fields if missing, keep CSS up-to-date
        try:
            existing_fields = _anki_request("modelFieldNames", modelName=_ANKI_MODEL) or []
            for fname in fields:
                if fname not in existing_fields:
                    _anki_request("modelFieldAdd",
                                  modelName=_ANKI_MODEL, fieldName=fname)
            _anki_request("updateModelStyling",
                          model={"name": _ANKI_MODEL, "css": _ANKI_CSS})
            _anki_request("updateModelTemplates",
                          model={"name": _ANKI_MODEL, "templates": {
                              _ANKI_TEMPLATES[0]["Name"]: {
                                  "Front": _ANKI_TEMPLATES[0]["Front"],
                                  "Back":  _ANKI_TEMPLATES[0]["Back"],
                              }
                          }})
        except Exception:
            pass


# ------------------------------------------------------------------ AnkiExportWorker

def _build_meaning_html(defs: list) -> str:
    """Build grouped definitions HTML from [(src_label, text), ...] pairs."""
    import hashlib, html, re
    _known_dicts = {'明鏡', '小学館'}
    groups: dict = {}
    for src, txt in defs:
        groups.setdefault(src or '', []).append(txt)
    parts = ["<div class='defs'>"]
    palette = [
        "#c0392b", "#2471a3", "#16a085", "#8e44ad", "#d35400",
        "#2c7a7b", "#6c5ce7", "#b03a5b", "#4b6584", "#5f8f3f",
    ]

    def _hdr_style(src: str) -> str:
        if src in _known_dicts:
            return ""
        h = int(hashlib.md5(src.encode("utf-8")).hexdigest()[:8], 16)
        return f" style='background:{palette[h % len(palette)]}'"

    def _render_item(item) -> str:
        if isinstance(item, dict) and item.get("html"):
            return str(item["html"])
        text = html.escape(_clean_def_text(str(item)))
        m = re.match(r"^(【[^】]+】)?([①②③④⑤⑥⑦⑧⑨⑩⑪⑫⑬⑭⑮⑯⑰⑱⑲⑳]|[0-9０-９]+)(?:〔([^〕]+)〕)?(.*)$", text)
        if m:
            prefix = m.group(1) or ""
            num = m.group(2)
            tag = m.group(3) or ""
            body = m.group(4).strip()
            prefix_html = f"<div class='ym-term-ref'>{prefix}</div>" if prefix else ""
            tag_html = f"<span class='ym-tag'>〔{tag}〕</span>" if tag else ""
            return (
                f"{prefix_html}<div class='ym-sense'>"
                f"<span class='ym-index'>{num}</span>{tag_html}"
                f"<span class='ym-gloss'>{body}</span></div>"
            )
        note = re.match(r"^(\[[^\]]+\])(.*)$", text)
        if note:
            return (
                "<div class='ym-note'>"
                f"<span class='ym-note-label'>{note.group(1)}</span>"
                f"<span class='ym-note-body'>{note.group(2).strip()}</span>"
                "</div>"
            )
        return f"<div class='ym-sense'><span class='ym-gloss'>{text}</span></div>"

    for src, items in groups.items():
        hdr_cls = f"dict-hdr-{src}" if src in _known_dicts else "dict-hdr-other"
        hdr = (f"<div class='dict-group'>"
               f"<span class='dict-hdr {hdr_cls}'{_hdr_style(src)}>{html.escape(src)}</span>"
               if src else "<div class='dict-group'>")
        parts.append(hdr)
        parts.append("<div class='meaning'>")
        for item in items:
            parts.append(f"<div class='ym-item'>{_render_item(item)}</div>")
        parts.append("</div></div>")
    parts.append("</div>")
    return "".join(parts)


class AnkiExportWorker(QThread):
    progress      = pyqtSignal(int, int, str)   # done, total, current_lemma
    card_done     = pyqtSignal(str, str)         # lemma, status  ("✅追加"/"⏭跳过"/"🔄更新"/"❌<reason>")
    finished      = pyqtSignal(bool, str)
    review_needed = pyqtSignal(str, str, list)

    def __init__(self, words: list, cfg: dict):
        import threading
        super().__init__()
        self.words = words
        self.cfg   = cfg
        self._review_event  = threading.Event()
        self._review_result: list = []

    def set_review_result(self, approved: list):
        self._review_result = approved
        self._review_event.set()

    def run(self):
        try:
            self._run_inner()
        except Exception as e:
            import traceback
            self.finished.emit(False, f"予期しないエラー: {e}\n{traceback.format_exc()}")

    def _run_inner(self):
        import time, base64, tempfile
        added = skipped = updated = 0
        errors: list[tuple[str, str]] = []   # (lemma, reason)

        try:
            _ensure_anki_model()
        except Exception as e:
            self.finished.emit(False, f"モデル作成エラー: {e}")
            return

        deck        = self.cfg["deck"]
        max_ex      = self.cfg.get("max_examples", 2)
        do_def      = self.cfg.get("fetch_def", True)
        do_audio    = self.cfg.get("clip_audio", False)
        dup_mode    = self.cfg.get("dup_mode", "手动选择")
        dup_scope   = self.cfg.get("dup_scope", "当前牌组")
        song_clause = self.cfg.get("song_clause", "")
        song_params = self.cfg.get("song_params", [])
        jlpt_map    = self.cfg.get("jlpt_map", {})
        # Deck used for duplicate lookups
        check_deck  = deck.split("::")[0] if dup_scope == "主牌组" else deck
        dup_opts    = {"allowDuplicate": False, "duplicateScope": "deck"}
        if check_deck != deck:
            dup_opts["duplicateScopeOptions"] = {
                "deckName": check_deck, "checkChildren": True}

        # ── Validate deck exists before starting ──────────────────────
        try:
            all_decks = _anki_request("deckNames") or []
            if deck not in all_decks:
                self.finished.emit(False,
                    f"牌组「{deck}」在 Anki 中不存在。\n\n"
                    f"Anki 中现有牌组：\n" + "\n".join(f"  • {d}" for d in sorted(all_decks)))
                return
        except (ConnectionError, OSError) as e:
            self.finished.emit(False, f"AnkiConnect 接続エラー: {e}")
            return

        media_dir = pathlib.Path(tempfile.gettempdir()) / "jpop_anki_clips"
        media_dir.mkdir(exist_ok=True)
        conn = sqlite3.connect(str(DB_PATH))
        total = len(self.words)

        for i, word in enumerate(self.words):
            lemma = word["lemma"]
            self.progress.emit(i, total, lemma)
            try:
                def_lang  = self.cfg.get("def_lang", "英文")
                jisho = _lookup_word_for_anki(
                    lemma,
                    word.get("pos", ""),
                    def_lang,
                    word.get("surface", ""),
                    do_def,
                )
                if do_def and not _has_local_dicts():
                    time.sleep(0.3)
                reading      = jisho.get("reading", "")
                defs         = jisho.get("defs", [])
                jlpt_str     = (jlpt_map.get(lemma) or
                                (jisho.get("jlpt", [""])[0].upper()
                                 if jisho.get("jlpt") else ""))
                pitch_str    = _lookup_yomitan_pitch(lemma, reading)
                lookup_term   = jisho.get("lookup_term", lemma)
                if lookup_term != lemma:
                    pitch_str = _lookup_yomitan_pitch(lookup_term, reading) or pitch_str
                freq_str     = _lookup_yomitan_freq(lemma) or _lookup_yomitan_freq(lookup_term)
                meaning_html = _prefix_lookup_term_html(
                    _build_meaning_html(defs), lemma, lookup_term)

                rows = conn.execute(
                    f"""
                    SELECT u.id, s.artist, s.title, u.time_sec, u.text, s.audio_path, t.surface,
                        (SELECT MIN(u2.time_sec) FROM utterances u2
                         WHERE u2.song_id = u.song_id AND u2.time_sec > u.time_sec) AS end_sec
                    FROM tokens t
                    JOIN utterances u ON u.id = t.utterance_id
                    JOIN songs s ON s.id = u.song_id
                    WHERE t.lemma = ? {song_clause}
                    ORDER BY RANDOM()
                    LIMIT ?
                    """,
                    [lemma] + song_params + [max_ex * 4]
                ).fetchall()
                # Deduplicate identical lyric lines, then trim to max_ex
                _seen_texts: set = set()
                deduped = []
                for _r in rows:
                    if _r[4] not in _seen_texts:
                        _seen_texts.add(_r[4])
                        deduped.append(_r)
                        if len(deduped) >= max_ex:
                            break
                rows = deduped

                sent_parts, audio_refs = [], []
                for utt_id, artist, title, t_sec, text, audio_path, surface, end_sec in rows:
                    target = surface if (surface and surface in text) else lemma
                    hi = text.replace(target,
                        f'<b style="color:#c0392b">{target}</b>', 1)
                    mins, secs = int(t_sec // 60), t_sec % 60
                    sent_parts.append(
                        f'<div class="sent">{hi}'
                        f'<span class="sent-src">'
                        f'{artist}「{title}」{mins:02d}:{secs:04.1f}</span></div>'
                    )
                    if do_audio and audio_path:
                        fname = f"jpop_{utt_id}.mp3"
                        ok = _clip_audio(audio_path, t_sec,
                                         end_sec or t_sec + 5.0,
                                         str(media_dir / fname))
                        if ok:
                            try:
                                with open(media_dir / fname, "rb") as f:
                                    b64 = base64.b64encode(f.read()).decode()
                                _anki_request("storeMediaFile",
                                              filename=fname, data=b64)
                                audio_refs.append(f"[sound:{fname}]")
                            except Exception:
                                pass

                sources = list({f"{r[1]}「{r[2]}」" for r in rows})
                pos_ja  = _POS_JA.get(word.get("pos", ""), "")
                note = {
                    "deckName": deck,
                    "modelName": _ANKI_MODEL,
                    "fields": {
                        "Expression":    lemma,
                        "Reading":       reading,
                        "Meaning":       meaning_html,
                        "Sentence":      "".join(sent_parts),
                        "SentenceAudio": " ".join(audio_refs),
                        "Source":        "、".join(sources),
                        "JLPT":          jlpt_str,
                        "Pitch":         pitch_str,
                        "Freq":          freq_str,
                        "PartOfSpeech":  pos_ja,
                    },
                    "options": dup_opts,
                    "tags": ["jpop-corpus"],
                }
                _anki_request("addNote", note=note)
                added += 1
                self.card_done.emit(lemma, "✅ 追加")

            except (ConnectionError, OSError) as e:
                conn.close()
                self.finished.emit(False,
                    f"AnkiConnect 接続エラー（Anki が起動しているか確認）: {e}")
                return
            except RuntimeError as e:
                if "duplicate" in str(e).lower():
                    if dup_mode in ("自动追加", "手动选择"):
                        try:
                            ids = _anki_request(
                                "findNotes",
                                query=f'deck:"{check_deck}" Expression:"{lemma}"')
                            if ids:
                                nid = ids[0]
                                info = _anki_request("notesInfo", notes=[nid])[0]
                                old_sent = info["fields"].get("Sentence", {}).get("value", "")
                                new_blocks = [b for b in sent_parts if b not in old_sent]
                                if not new_blocks:
                                    skipped += 1
                                    self.card_done.emit(lemma, "⏭ 跳过（例句无变化）")
                                elif dup_mode == "自动追加":
                                    _anki_request("updateNoteFields", note={
                                        "id": nid,
                                        "fields": {"Sentence": old_sent + "".join(new_blocks)}
                                    })
                                    updated += 1
                                    self.card_done.emit(lemma, "🔄 更新例句")
                                else:  # 手动选择
                                    self._review_event.clear()
                                    self._review_result = []
                                    self.review_needed.emit(lemma, old_sent, new_blocks)
                                    self._review_event.wait()
                                    approved = self._review_result
                                    if approved:
                                        _anki_request("updateNoteFields", note={
                                            "id": nid,
                                            "fields": {"Sentence": old_sent + "".join(approved)}
                                        })
                                        updated += 1
                                        self.card_done.emit(lemma, "🔄 更新例句")
                                    else:
                                        skipped += 1
                                        self.card_done.emit(lemma, "⏭ 跳过（用户取消）")
                            else:
                                skipped += 1
                                self.card_done.emit(lemma, "⏭ 跳过（重复但未找到原卡）")
                        except Exception as ie:
                            skipped += 1
                            self.card_done.emit(lemma, f"⏭ 跳过（{ie}）")
                    else:
                        skipped += 1
                        self.card_done.emit(lemma, "⏭ 跳过（重复词）")
                else:
                    reason = str(e)
                    errors.append((lemma, reason))
                    self.card_done.emit(lemma, f"❌ {reason}")
            except Exception as e:
                reason = str(e) or type(e).__name__
                errors.append((lemma, reason))
                self.card_done.emit(lemma, f"❌ {reason}")

        conn.close()
        self.progress.emit(total, total, "完了")
        parts = [f"追加: {added}"]
        if updated: parts.append(f"例句更新: {updated}")
        if skipped: parts.append(f"跳过: {skipped}")
        if errors:  parts.append(f"失败: {len(errors)}")
        # Attach error detail as trailing lines so UI can show them
        summary = "完了 — " + "  ".join(parts)
        if errors:
            detail = "\n".join(f"  {lm}：{r}" for lm, r in errors)
            summary += f"\n\n失败明细：\n{detail}"
        self.finished.emit(True, summary)


# ------------------------------------------------------------------ AnkiUpdateWorker

class AnkiUpdateWorker(QThread):
    """Updates Reading/Meaning/JLPT/Pitch/Freq of existing Anki notes (leaves Sentence intact)."""
    progress = pyqtSignal(int, int, str)
    finished = pyqtSignal(bool, str)

    def __init__(self, words: list, cfg: dict):
        super().__init__()
        self.words = words
        self.cfg   = cfg

    def run(self):
        try:
            self._run_inner()
        except Exception as e:
            import traceback
            self.finished.emit(False, f"予期しないエラー: {e}\n{traceback.format_exc()}")

    def _run_inner(self):
        import time
        updated = skipped = err_count = 0
        deck       = self.cfg["deck"]
        do_def     = self.cfg.get("fetch_def", True)
        def_lang   = self.cfg.get("def_lang", "英文")
        jlpt_map   = self.cfg.get("jlpt_map", {})
        dup_scope  = self.cfg.get("dup_scope", "当前牌组")
        check_deck = deck.split("::")[0] if dup_scope == "主牌组" else deck

        try:
            _ensure_anki_model()   # also refreshes CSS / template
        except Exception as e:
            self.finished.emit(False, f"モデル更新エラー: {e}")
            return

        for i, word in enumerate(self.words):
            lemma = word["lemma"]
            self.progress.emit(i, len(self.words), lemma)
            try:
                ids = _anki_request("findNotes",
                                    query=f'deck:"{check_deck}" Expression:"{lemma}"')
                if not ids:
                    skipped += 1
                    continue

                jisho = _lookup_word_for_anki(
                    lemma,
                    word.get("pos", ""),
                    def_lang,
                    word.get("surface", ""),
                    do_def,
                )
                if do_def and not _has_local_dicts():
                    time.sleep(0.3)

                reading      = jisho.get("reading", "")
                defs         = jisho.get("defs", [])
                jlpt_str     = (jlpt_map.get(lemma) or
                                (jisho.get("jlpt", [""])[0].upper()
                                 if jisho.get("jlpt") else ""))
                pitch_str    = _lookup_yomitan_pitch(lemma, reading)
                lookup_term   = jisho.get("lookup_term", lemma)
                if lookup_term != lemma:
                    pitch_str = _lookup_yomitan_pitch(lookup_term, reading) or pitch_str
                freq_str     = _lookup_yomitan_freq(lemma) or _lookup_yomitan_freq(lookup_term)
                meaning_html = _prefix_lookup_term_html(
                    _build_meaning_html(defs), lemma, lookup_term)

                pos_ja = _POS_JA.get(word.get("pos", ""), "")
                _anki_request("updateNoteFields", note={
                    "id": ids[0],
                    "fields": {
                        "Reading":      reading,
                        "Meaning":      meaning_html,
                        "JLPT":         jlpt_str,
                        "Pitch":        pitch_str,
                        "Freq":         freq_str,
                        "PartOfSpeech": pos_ja,
                    }
                })
                updated += 1
            except (ConnectionError, OSError) as e:
                self.finished.emit(False, f"AnkiConnect 接続エラー: {e}")
                return
            except Exception:
                err_count += 1

        self.progress.emit(len(self.words), len(self.words), "完了")
        parts = [f"更新: {updated}"]
        if skipped:
            parts.append(f"未找到: {skipped}")
        if err_count:
            parts.append(f"エラー: {err_count}")
        self.finished.emit(True, "完了 — " + "  ".join(parts))


_ANKI_FIELD_TAG_RE = re.compile(r"<[^>]+>")


def _plain_anki_field(raw: str) -> str:
    return _ANKI_FIELD_TAG_RE.sub("", html.unescape(raw or "")).strip()


def _anki_query_quote(text: str) -> str:
    return (text or "").replace("\\", "\\\\").replace('"', r'\"')


def _lookup_common_pos_map(lemmas: list[str]) -> dict[str, str]:
    if not lemmas:
        return {}
    result: dict[str, str] = {}
    unique = list(dict.fromkeys(l for l in lemmas if l))
    try:
        conn = sqlite3.connect(str(DB_PATH))
        for start in range(0, len(unique), 500):
            chunk = unique[start:start + 500]
            ph = ",".join("?" * len(chunk))
            rows = conn.execute(
                f"""
                SELECT lemma, pos, COUNT(*) AS cnt
                FROM tokens
                WHERE lemma IN ({ph})
                GROUP BY lemma, pos
                ORDER BY lemma, cnt DESC
                """,
                chunk,
            ).fetchall()
            for lemma, pos, _cnt in rows:
                result.setdefault(lemma, pos)
        conn.close()
    except Exception:
        pass
    return result


class AnkiDeckRefreshWorker(QThread):
    """Refresh every JPOP Corpus note in a deck with current dictionary fields."""
    progress = pyqtSignal(int, int, str)
    card_done = pyqtSignal(str, str)
    finished = pyqtSignal(bool, str)

    def __init__(self, cfg: dict):
        super().__init__()
        self.cfg = cfg

    def run(self):
        try:
            self._run_inner()
        except Exception as e:
            import traceback
            self.finished.emit(False, f"予期しないエラー: {e}\n{traceback.format_exc()}")

    def _deck_matches_scope(self, card_deck: str, selected_deck: str, refresh_scope: str) -> bool:
        if refresh_scope == "主牌组+子牌组":
            root = selected_deck.split("::")[0]
            return card_deck == root or card_deck.startswith(root + "::")
        if refresh_scope == "当前牌组+子牌组":
            return card_deck == selected_deck or card_deck.startswith(selected_deck + "::")
        return card_deck == selected_deck

    def _find_target_notes(self, deck: str, refresh_scope: str) -> list[int]:
        model_q = _anki_query_quote(_ANKI_MODEL)
        queries = [f'note:"{model_q}"', "tag:jpop-corpus"]
        card_ids: list[int] = []
        seen_cards: set[int] = set()
        seen_notes: set[int] = set()
        seen: set[int] = set()
        ids: list[int] = []
        for query in queries:
            try:
                found = _anki_request("findCards", query=query) or []
            except Exception:
                found = []
            for cid in found:
                if cid not in seen_cards:
                    seen_cards.add(cid)
                    card_ids.append(cid)

        for start in range(0, len(card_ids), 100):
            infos = _anki_request("cardsInfo", cards=card_ids[start:start + 100]) or []
            for info in infos:
                card_deck = info.get("deckName", "")
                nid = info.get("note") or info.get("noteId")
                if not nid:
                    continue
                if not self._deck_matches_scope(card_deck, deck, refresh_scope):
                    continue
                if nid not in seen_notes:
                    seen_notes.add(nid)
                    ids.append(int(nid))
        return ids

    def _run_inner(self):
        import time
        deck = self.cfg["deck"]
        do_def = self.cfg.get("fetch_def", True)
        def_lang = self.cfg.get("def_lang", "中文")
        refresh_scope = self.cfg.get("refresh_scope", "仅当前牌组")
        target_label = {
            "主牌组+子牌组": deck.split("::")[0],
            "当前牌组+子牌组": deck + " 及其子牌组",
        }.get(refresh_scope, deck)

        try:
            _ensure_anki_model()
            all_decks = _anki_request("deckNames") or []
            if deck not in all_decks:
                self.finished.emit(False, f"牌组「{deck}」在 Anki 中不存在。")
                return
            note_ids = self._find_target_notes(deck, refresh_scope)
        except (ConnectionError, OSError) as e:
            self.finished.emit(False, f"AnkiConnect 接続エラー: {e}")
            return
        except Exception as e:
            self.finished.emit(False, f"旧卡扫描失败: {e}")
            return

        if not note_ids:
            self.finished.emit(True, f"完了 — 在「{target_label}」中未找到 JPOP Corpus 旧卡")
            return

        try:
            infos = []
            for start in range(0, len(note_ids), 100):
                infos.extend(_anki_request("notesInfo", notes=note_ids[start:start + 100]) or [])
        except Exception as e:
            self.finished.emit(False, f"读取旧卡信息失败: {e}")
            return

        note_words: list[tuple[int, str]] = []
        for info in infos:
            fields = info.get("fields", {}) if isinstance(info, dict) else {}
            expr = _plain_anki_field(fields.get("Expression", {}).get("value", ""))
            nid = info.get("noteId") or info.get("id")
            if nid and expr:
                note_words.append((int(nid), expr))

        if not note_words:
            self.finished.emit(True, "完了 — 找到旧卡，但没有可读取的 Expression 字段")
            return

        pos_map = _lookup_common_pos_map([lemma for _, lemma in note_words])
        jlpt_map = self.cfg.get("jlpt_map", {})
        updated = skipped = err_count = 0
        total = len(note_words)

        for i, (nid, lemma) in enumerate(note_words):
            self.progress.emit(i, total, lemma)
            try:
                pos = pos_map.get(lemma, "")
                jisho = _lookup_word_for_anki(lemma, pos, def_lang, "", do_def)
                if do_def and not _has_local_dicts():
                    time.sleep(0.3)
                reading = jisho.get("reading", "")
                defs = jisho.get("defs", [])
                jlpt_str = (
                    jlpt_map.get(lemma) or
                    (jisho.get("jlpt", [""])[0].upper() if jisho.get("jlpt") else "")
                )
                pitch_str = _lookup_yomitan_pitch(lemma, reading)
                lookup_term = jisho.get("lookup_term", lemma)
                if lookup_term != lemma:
                    pitch_str = _lookup_yomitan_pitch(lookup_term, reading) or pitch_str
                freq_str = _lookup_yomitan_freq(lemma) or _lookup_yomitan_freq(lookup_term)
                pos_ja = _POS_JA.get(pos, "")
                fields = {
                    "Reading": reading,
                    "Meaning": _prefix_lookup_term_html(
                        _build_meaning_html(defs), lemma, lookup_term),
                    "JLPT": jlpt_str,
                    "Pitch": pitch_str,
                    "Freq": freq_str,
                    "PartOfSpeech": pos_ja,
                }
                _anki_request("updateNoteFields", note={"id": nid, "fields": fields})
                updated += 1
                self.card_done.emit(lemma, "🔄 已刷新旧卡")
            except (ConnectionError, OSError) as e:
                self.finished.emit(False, f"AnkiConnect 接続エラー: {e}")
                return
            except Exception as e:
                err_count += 1
                self.card_done.emit(lemma, f"❌ {e}")

        self.progress.emit(total, total, "完了")
        parts = [f"刷新: {updated}"]
        if skipped:
            parts.append(f"跳过: {skipped}")
        if err_count:
            parts.append(f"失败: {err_count}")
        self.finished.emit(True, "完了 — " + "  ".join(parts))


class AnkiLearningSyncWorker(QThread):
    """Read Anki learning state without blocking the export dialog."""
    done = pyqtSignal(object, str, str)  # status_map, collection_path, error

    def run(self):
        try:
            status, path = load_learning_status()
            self.done.emit(status, str(path), "")
        except Exception as e:
            self.done.emit({}, "", str(e))


# ------------------------------------------------------------------ SentenceReviewDialog

class SentenceReviewDialog(QDialog):
    """Shown when a duplicate word is found; user picks which new sentences to append."""

    def __init__(self, lemma: str, existing_html: str, candidates: list, parent=None):
        super().__init__(parent)
        self.setWindowTitle(f"例句审核 — {lemma}")
        self.setMinimumWidth(640)
        self.setMinimumHeight(420)
        self._candidates = candidates
        self._chks: list = []
        self._build_ui(lemma, existing_html, candidates)

    def _build_ui(self, lemma, existing_html, candidates):
        layout = QVBoxLayout(self)

        layout.addWidget(QLabel(f"<b>{lemma}</b> 已有卡片，请选择要追加的例句："))

        # Existing sentences (read-only preview)
        if existing_html.strip():
            grp_ex = QGroupBox("已有例句")
            ex_lay = QVBoxLayout(grp_ex)
            ex_lbl = QLabel(existing_html)
            ex_lbl.setWordWrap(True)
            ex_lbl.setTextFormat(Qt.TextFormat.RichText)
            ex_lbl.setStyleSheet("font-size:12px; color:#888;")
            ex_scroll = QScrollArea()
            ex_scroll.setWidget(ex_lbl)
            ex_scroll.setWidgetResizable(True)
            ex_scroll.setMaximumHeight(120)
            ex_lay.addWidget(ex_scroll)
            layout.addWidget(grp_ex)

        # Candidate sentences (checkboxes)
        grp_new = QGroupBox("新例句（勾选要追加的）")
        new_lay = QVBoxLayout(grp_new)
        for html in candidates:
            # Strip tags for checkbox label
            import re as _re
            plain = _re.sub(r'<[^>]+>', '', html).strip()
            chk = QCheckBox(plain)
            chk.setChecked(True)
            chk.setProperty("html", html)
            self._chks.append(chk)
            new_lay.addWidget(chk)
        layout.addWidget(grp_new)

        # Buttons
        btn_row = QHBoxLayout()
        btn_all  = QPushButton("全选")
        btn_none = QPushButton("全不选")
        btn_all.clicked.connect(lambda: [c.setChecked(True)  for c in self._chks])
        btn_none.clicked.connect(lambda: [c.setChecked(False) for c in self._chks])
        btn_row.addWidget(btn_all)
        btn_row.addWidget(btn_none)
        btn_row.addStretch()
        btn_ok     = QPushButton("确认追加")
        btn_skip   = QPushButton("跳过此词")
        btn_ok.setDefault(True)
        btn_ok.clicked.connect(self.accept)
        btn_skip.clicked.connect(self.reject)
        btn_row.addWidget(btn_ok)
        btn_row.addWidget(btn_skip)
        layout.addLayout(btn_row)

    def approved_parts(self) -> list:
        return [c.property("html") for c in self._chks if c.isChecked()]


def _ensure_jlpt_cache_table():
    conn = sqlite3.connect(str(DB_PATH))
    conn.execute(
        "CREATE TABLE IF NOT EXISTS jlpt_cache "
        "(lemma TEXT PRIMARY KEY, level TEXT NOT NULL DEFAULT '')"
    )
    conn.commit()
    conn.close()


def _download_jlpt_static() -> bool:
    """
    One-time download of JLPT N5-N1 word lists from Jisho API (paginated,
    ~750 pages total, fetched with 20 concurrent threads). Stores all words
    (kanji + kana forms) in jlpt_cache. Returns True if ≥ 5000 entries added.
    """
    import urllib.request, json as _json
    from concurrent.futures import ThreadPoolExecutor, as_completed

    _ensure_jlpt_cache_table()
    conn = sqlite3.connect(str(DB_PATH))
    try:
        if conn.execute("SELECT COUNT(*) FROM jlpt_cache").fetchone()[0] >= 5000:
            conn.close()
            return True
    except Exception:
        conn.close()
        return False

    # Known max pages per level (safe upper bound)
    level_pages = {"n5": 40, "n4": 80, "n3": 200, "n2": 80, "n1": 200}
    batch: list[tuple[str, str]] = []

    def _fetch_page(lvl: str, page: int) -> list[tuple[str, str]]:
        url = (f"https://jisho.org/api/v1/search/words"
               f"?keyword=%23jlpt-{lvl}&page={page}")
        try:
            req = urllib.request.Request(
                url, headers={"User-Agent": "Mozilla/5.0"})
            with urllib.request.urlopen(req, timeout=10) as r:
                items = _json.loads(r.read()).get("data", [])
        except Exception:
            return []
        words = []
        lvl_str = lvl.upper()
        for item in items:
            for jp in item.get("japanese", []):
                w = jp.get("word", "")
                r = jp.get("reading", "")
                if w:
                    words.append((w, lvl_str))
                if r and r != w:
                    words.append((r, lvl_str))
        return words

    futures = {}
    with ThreadPoolExecutor(max_workers=20) as ex:
        for lvl, max_p in level_pages.items():
            for p in range(1, max_p + 1):
                futures[ex.submit(_fetch_page, lvl, p)] = (lvl, p)
        for fut in as_completed(futures):
            batch.extend(fut.result())

    try:
        conn.executemany(
            "INSERT OR IGNORE INTO jlpt_cache (lemma, level) VALUES (?,?)", batch)
        conn.commit()
        added = conn.execute("SELECT COUNT(*) FROM jlpt_cache").fetchone()[0]
        conn.close()
        return added >= 5000
    except Exception:
        conn.close()
        return False


# ------------------------------------------------------------------ JlptFetchWorker

class JlptFetchWorker(QThread):
    """Background worker: fetches JLPT levels (DB cache first, Jisho fallback)."""
    result = pyqtSignal(str, str)   # lemma, jlpt_level ("N5"/"N4"/… or "")
    done   = pyqtSignal()

    def __init__(self, lemmas: list):
        super().__init__()
        self.lemmas = lemmas

    def run(self):
        import time
        # Load cached levels in one query
        try:
            _ensure_jlpt_cache_table()
            conn = sqlite3.connect(str(DB_PATH))
            ph = ",".join("?" * len(self.lemmas))
            cached = dict(conn.execute(
                f"SELECT lemma, level FROM jlpt_cache WHERE lemma IN ({ph})",
                self.lemmas
            ).fetchall())
            conn.close()
        except Exception:
            cached = {}

        for lemma in self.lemmas:
            if self.isInterruptionRequested():
                break
            if lemma in cached:
                self.result.emit(lemma, cached[lemma])
                continue
            # Not cached – call Jisho and store
            r = _jisho_lookup(lemma)
            jlpt_list = r.get("jlpt", [])
            # Normalize: Jisho returns "jlpt-n3" → we want "N3"
            raw = jlpt_list[0].upper() if jlpt_list else ""
            level = raw.replace("JLPT-", "") if raw.startswith("JLPT-") else raw
            self.result.emit(lemma, level)
            try:
                c = sqlite3.connect(str(DB_PATH))
                c.execute(
                    "INSERT OR REPLACE INTO jlpt_cache (lemma, level) VALUES (?,?)",
                    (lemma, level)
                )
                c.commit()
                c.close()
            except Exception:
                pass
            time.sleep(0.08)
        self.done.emit()


# ------------------------------------------------------------------ Token Correction

def _ensure_token_corrections_table():
    conn = sqlite3.connect(str(DB_PATH))
    conn.execute("""
        CREATE TABLE IF NOT EXISTS token_corrections (
            utterance_id INTEGER PRIMARY KEY,
            tokens_json  TEXT NOT NULL,
            orig_json    TEXT NOT NULL DEFAULT '[]',
            text         TEXT NOT NULL DEFAULT '',
            song_artist  TEXT NOT NULL DEFAULT '',
            song_title   TEXT NOT NULL DEFAULT ''
        )
    """)
    cols = {r[1] for r in conn.execute("PRAGMA table_info(token_corrections)").fetchall()}
    for col, ddl in [
        ('orig_json',   "ALTER TABLE token_corrections ADD COLUMN orig_json   TEXT NOT NULL DEFAULT '[]'"),
        ('text',        "ALTER TABLE token_corrections ADD COLUMN text        TEXT NOT NULL DEFAULT ''"),
        ('song_artist', "ALTER TABLE token_corrections ADD COLUMN song_artist TEXT NOT NULL DEFAULT ''"),
        ('song_title',  "ALTER TABLE token_corrections ADD COLUMN song_title  TEXT NOT NULL DEFAULT ''"),
    ]:
        if col not in cols:
            conn.execute(ddl)
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_tc_song_text "
        "ON token_corrections(song_artist, song_title, text)"
    )
    conn.commit()
    conn.close()


def _apply_token_correction(utterance_id: int, tokens: list, orig_tokens: list,
                             *, text: str = "", artist: str = "", title: str = ""):
    """Overwrite tokens table entry and persist correction + original + text key."""
    conn = sqlite3.connect(str(DB_PATH))
    try:
        existing = conn.execute(
            "SELECT orig_json FROM token_corrections WHERE utterance_id=?",
            (utterance_id,)).fetchone()
        orig_json = (existing[0] if existing and existing[0] != '[]'
                     else json.dumps(orig_tokens, ensure_ascii=False))
        conn.execute(
            "INSERT OR REPLACE INTO token_corrections "
            "(utterance_id, tokens_json, orig_json, text, song_artist, song_title) "
            "VALUES (?,?,?,?,?,?)",
            (utterance_id,
             json.dumps(tokens, ensure_ascii=False),
             orig_json, text, artist, title))
        conn.execute("DELETE FROM tokens WHERE utterance_id=?", (utterance_id,))
        conn.executemany(
            "INSERT INTO tokens(utterance_id,token_idx,surface,lemma,pos) VALUES (?,?,?,?,?)",
            [(utterance_id, i, t['surface'], t.get('lemma', t['surface']), t.get('pos', 'NOUN'))
             for i, t in enumerate(tokens)])
        conn.commit()
    finally:
        conn.close()




# Token correction dialog. moved to dialogs/token_correction.py




# Yomitan dictionary manager dialog. moved to dialogs/dict_manager.py




# Anki export dialog. moved to dialogs/anki_export.py


class CorpusReportWorker(QThread):
    """后台计算语料统计报告：TTR / STTR / Hapax / 词性分布 / 覆盖率。"""
    finished = pyqtSignal(dict)

    def __init__(self, artist_filter: list, jp_only: bool):
        super().__init__()
        self.artist_filter = artist_filter
        self.jp_only = jp_only

    def run(self):
        artist_clause = ""
        params = []
        if self.artist_filter:
            conds = " OR ".join(
                ["'/' || s.artist || '/' LIKE '%/' || ? || '/%'"] * len(self.artist_filter)
            )
            artist_clause = f"AND ({conds})"
            params = list(self.artist_filter)

        conn = sqlite3.connect(str(DB_PATH))

        song_count = conn.execute(
            f"SELECT COUNT(DISTINCT s.id) FROM songs s "
            f"JOIN utterances u ON u.song_id = s.id WHERE 1=1 {artist_clause}",
            params
        ).fetchone()[0]

        utt_count = conn.execute(
            f"SELECT COUNT(u.id) FROM utterances u "
            f"JOIN songs s ON s.id = u.song_id WHERE 1=1 {artist_clause}",
            params
        ).fetchone()[0]

        # 词性分布（含各 POS 的 token 数和 type 数）
        pos_rows = conn.execute(
            f"""SELECT t.pos, COUNT(*) AS tc, COUNT(DISTINCT t.lemma) AS dc
                FROM tokens t
                JOIN utterances u ON u.id = t.utterance_id
                JOIN songs s ON s.id = u.song_id
                WHERE t.pos NOT IN ('PUNCT','SYM','SPACE','X') {artist_clause}
                GROUP BY t.pos ORDER BY tc DESC""",
            params
        ).fetchall()
        token_count = sum(r[1] for r in pos_rows)

        # lemma 频率分布（用于 TTR / Hapax / 覆盖率）
        freq_rows = conn.execute(
            f"""SELECT t.lemma, COUNT(*) AS freq
                FROM tokens t
                JOIN utterances u ON u.id = t.utterance_id
                JOIN songs s ON s.id = u.song_id
                WHERE t.pos NOT IN ('PUNCT','SYM','SPACE','X') {artist_clause}
                GROUP BY t.lemma ORDER BY freq DESC""",
            params
        ).fetchall()

        if self.jp_only:
            freq_rows = [(l, f) for l, f in freq_rows if _JP_RE.search(l)]

        type_count = len(freq_rows)
        ttr = type_count / token_count if token_count > 0 else 0.0
        hapax_count = sum(1 for _, f in freq_rows if f == 1)
        hapax_ratio = hapax_count / type_count if type_count > 0 else 0.0

        # Top-N 覆盖率
        cum = 0
        n_targets = [100, 500, 1000, 2000]
        coverage, n_idx = [], 0
        for i, (_, freq) in enumerate(freq_rows):
            cum += freq
            while n_idx < len(n_targets) and (i + 1) >= n_targets[n_idx]:
                n = n_targets[n_idx]
                coverage.append({"n": n, "pct": cum / token_count * 100 if token_count else 0})
                n_idx += 1
        while n_idx < len(n_targets):
            coverage.append({"n": n_targets[n_idx], "pct": cum / token_count * 100 if token_count else 0})
            n_idx += 1

        # STTR（标准化 TTR）：将 token 序列切成等长片段，取平均 TTR
        sttr, chunk_size = None, None
        if token_count >= 5000:
            chunk_size = 1000
        elif token_count >= 1000:
            chunk_size = 500
        if chunk_size:
            all_lemmas = conn.execute(
                f"""SELECT t.lemma FROM tokens t
                    JOIN utterances u ON u.id = t.utterance_id
                    JOIN songs s ON s.id = u.song_id
                    WHERE t.pos NOT IN ('PUNCT','SYM','SPACE','X') {artist_clause}
                    ORDER BY s.id, u.line_idx, t.token_idx""",
                params
            ).fetchall()
            lemma_seq = [r[0] for r in all_lemmas]
            if self.jp_only:
                lemma_seq = [l for l in lemma_seq if _JP_RE.search(l)]
            chunks = [lemma_seq[i:i + chunk_size] for i in range(0, len(lemma_seq), chunk_size)]
            complete = [c for c in chunks if len(c) == chunk_size]
            if complete:
                sttr = sum(len(set(c)) / chunk_size for c in complete) / len(complete)

        # 平均曲词汇量（每首歌的唯一 lemma 数）
        per_song_rows = conn.execute(
            f"""SELECT u.song_id, COUNT(DISTINCT t.lemma)
                FROM tokens t
                JOIN utterances u ON u.id = t.utterance_id
                JOIN songs s ON s.id = u.song_id
                WHERE t.pos NOT IN ('PUNCT','SYM','SPACE','X') {artist_clause}
                GROUP BY u.song_id""",
            params
        ).fetchall()
        avg_types_per_song = (
            sum(r[1] for r in per_song_rows) / len(per_song_rows)
        ) if per_song_rows else 0.0

        conn.close()

        pos_dist = [
            {
                "pos": pos, "token_count": tc,
                "token_pct": tc / token_count * 100 if token_count else 0,
                "type_count": dc,
            }
            for pos, tc, dc in pos_rows
        ]

        self.finished.emit({
            "song_count": song_count,
            "utterance_count": utt_count,
            "token_count": token_count,
            "type_count": type_count,
            "ttr": ttr,
            "sttr": sttr,
            "sttr_chunk": chunk_size,
            "hapax_count": hapax_count,
            "hapax_ratio": hapax_ratio,
            "avg_types_per_song": avg_types_per_song,
            "avg_tokens_per_line": token_count / utt_count if utt_count else 0.0,
            "pos_dist": pos_dist,
            "coverage": coverage,
            "top_words": freq_rows[:20],
        })


AUDIO_EXTS = {".flac", ".mp3", ".wav", ".m4a", ".ogg"}


class RepairAudioDialog(QDialog):
    """音声ファイル欠損の修復ダイアログ。フォルダスキャン + 手動リンク対応。"""

    def __init__(self, missing: list, parent=None):
        # missing: [(id, artist, title, old_path), ...]
        super().__init__(parent)
        self.setWindowTitle(f"音声ファイル修復 — {len(missing)} 件")
        self.setMinimumWidth(660)
        self.setMinimumHeight(420)
        self._missing   = missing
        self._resolved: dict[str, str] = {}   # id -> new_path

        layout = QVBoxLayout(self)
        layout.addWidget(QLabel(
            f"以下の {len(missing)} 曲の音声ファイルが見つかりません。\n"
            "フォルダをスキャンして自動マッチング、または行を選択して手動でリンクしてください。"
        ))

        self._table = QTableWidget(len(missing), 3)
        self._table.setHorizontalHeaderLabels(["歌手", "曲名", "状態"])
        hh = self._table.horizontalHeader()
        hh.setSectionResizeMode(0, QHeaderView.ResizeMode.ResizeToContents)
        hh.setSectionResizeMode(1, QHeaderView.ResizeMode.Stretch)
        hh.setSectionResizeMode(2, QHeaderView.ResizeMode.ResizeToContents)
        self._table.setEditTriggers(QTableWidget.EditTrigger.NoEditTriggers)
        self._table.setSelectionBehavior(QTableWidget.SelectionBehavior.SelectRows)
        for i, (_, artist, title, _) in enumerate(missing):
            self._table.setItem(i, 0, QTableWidgetItem(artist))
            self._table.setItem(i, 1, QTableWidgetItem(title))
            self._table.setItem(i, 2, QTableWidgetItem("未解決"))
        layout.addWidget(self._table)

        btn_row = QHBoxLayout()
        btn_scan = QPushButton("フォルダをスキャンして自動マッチング")
        btn_scan.clicked.connect(self._scan_folder)
        btn_row.addWidget(btn_scan)
        btn_manual = QPushButton("選択した曲を手動でリンク")
        btn_manual.clicked.connect(self._manual_link)
        btn_row.addWidget(btn_manual)
        layout.addLayout(btn_row)

        self._lbl = QLabel("")
        layout.addWidget(self._lbl)

        btns = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel
        )
        btns.accepted.connect(self._apply)
        btns.rejected.connect(self.reject)
        layout.addWidget(btns)

    def _scan_folder(self):
        folder = QFileDialog.getExistingDirectory(self, "音声ファイルのフォルダを選択")
        if not folder:
            return
        audio_files = [
            p for p in pathlib.Path(folder).rglob("*")
            if p.suffix.lower() in AUDIO_EXTS
        ]
        matched = 0
        for i, (sid, artist, title, _) in enumerate(self._missing):
            if sid in self._resolved:
                continue
            for p in audio_files:
                stem = p.stem.lower()
                if title.lower() in stem or sid in stem:
                    self._mark_resolved(i, sid, str(p))
                    matched += 1
                    break
        self._lbl.setText(f"スキャン結果：{matched} 件マッチ、{len(self._missing)-matched} 件未解決")

    def _manual_link(self):
        row = self._table.currentRow()
        if row < 0:
            QMessageBox.information(self, "", "行を選択してから実行してください")
            return
        sid, artist, title, _ = self._missing[row]
        path, _ = QFileDialog.getOpenFileName(
            self, f"{artist}《{title}》の音声ファイル", "",
            "音声ファイル (*.flac *.mp3 *.wav *.m4a);;すべてのファイル (*)"
        )
        if path:
            self._mark_resolved(row, sid, path)

    def _mark_resolved(self, row: int, sid: str, path: str):
        self._resolved[sid] = path
        item = self._table.item(row, 2)
        item.setText(f"✓  {pathlib.Path(path).name}")
        item.setForeground(QBrush(QColor("#5dade2")))

    def _apply(self):
        if self._resolved:
            conn = sqlite3.connect(str(DB_PATH))
            conn.executemany(
                "UPDATE songs SET audio_path=? WHERE id=?",
                [(p, sid) for sid, p in self._resolved.items()]
            )
            conn.commit()
            conn.close()
            with open(CSV_PATH, encoding="utf-8-sig", newline="") as f:
                rows = list(csv.DictReader(f))
            for row in rows:
                if row["id"] in self._resolved:
                    row["audio_path"] = self._resolved[row["id"]]
            with open(CSV_PATH, "w", encoding="utf-8-sig", newline="") as f:
                w = csv.DictWriter(
                    f, fieldnames=["id","title","artist","year","album","genre","audio_path"]
                )
                w.writeheader()
                w.writerows(rows)
        self.accept()

    def resolved_count(self) -> int:
        return len(self._resolved)


class ReorganizeWorker(QThread):
    """全曲の音声ファイルを raw/audio/{歌手}/ に整理し、DB と CSV を更新する。"""
    progress = pyqtSignal(str)
    finished = pyqtSignal(bool, str)

    def run(self):
        try:
            conn = sqlite3.connect(str(DB_PATH))
            songs = conn.execute(
                "SELECT id, artist, title, audio_path FROM songs"
            ).fetchall()

            db_updates: list[tuple] = []
            moved = skipped = 0

            for song_id, artist, title, audio_path in songs:
                src = pathlib.Path(audio_path)
                if not src.exists():
                    skipped += 1
                    continue

                safe_artist = re.sub(r'[\\/:*?"<>|]', "-", artist)
                dest = AUDIO_DIR / safe_artist / f"{song_id}{src.suffix.lower()}"

                if src.resolve() == dest.resolve():
                    continue

                dest.parent.mkdir(parents=True, exist_ok=True)
                try:
                    src.relative_to(AUDIO_DIR)
                    src.rename(dest)          # AUDIO_DIR 内 → 移動
                except ValueError:
                    shutil.copy2(src, dest)   # 外部 → コピー

                db_updates.append((str(dest), song_id))
                moved += 1
                self.progress.emit(f"[{moved}] {artist}《{title}》整理中…")

            if db_updates:
                conn.executemany(
                    "UPDATE songs SET audio_path=? WHERE id=?", db_updates
                )
                conn.commit()
                id_to_path = {sid: p for p, sid in db_updates}
                with open(CSV_PATH, encoding="utf-8-sig", newline="") as f:
                    rows = list(csv.DictReader(f))
                for row in rows:
                    if row["id"] in id_to_path:
                        row["audio_path"] = id_to_path[row["id"]]
                with open(CSV_PATH, "w", encoding="utf-8-sig", newline="") as f:
                    w = csv.DictWriter(
                        f, fieldnames=["id","title","artist","year","album","genre","audio_path"]
                    )
                    w.writeheader()
                    w.writerows(rows)

            conn.close()
            self.finished.emit(
                True,
                f"整理完了：{moved} 曲移動・コピー、{skipped} 曲（音声なし）スキップ"
            )
        except Exception as e:
            self.finished.emit(False, f"エラー：{e}")


class AddSongWorker(QThread):
    """后台处理：搜LRC、GiNZA分词、写入DB和CSV。"""
    progress = pyqtSignal(str)
    finished = pyqtSignal(bool, str)

    def __init__(self, songs: list[dict]):
        super().__init__()
        self.songs = songs

    def run(self):
        try:
            import syncedlyrics
            import spacy

            conn = sqlite3.connect(str(DB_PATH))
            nlp = None
            added = []
            skipped = []

            for idx, s in enumerate(self.songs):
                self.progress.emit(
                    f"[{idx+1}/{len(self.songs)}] {s['artist']}《{s['title']}》処理中…"
                )

                dup = conn.execute(
                    "SELECT id FROM songs WHERE artist = ? AND title = ?",
                    (s["artist"], s["title"])
                ).fetchone()
                if dup:
                    skipped.append(f"{s['artist']}《{s['title']}》")
                    continue

                row = conn.execute("SELECT MAX(CAST(id AS INT)) FROM songs").fetchone()
                new_id = f"{(row[0] or 0) + 1:03d}"

                # 音声ファイルを raw/audio/{artist}/ に整理してコピー
                safe_artist = re.sub(r'[\\/:*?"<>|]', "-", s["artist"])
                artist_dir  = AUDIO_DIR / safe_artist
                artist_dir.mkdir(parents=True, exist_ok=True)
                ext  = pathlib.Path(s["audio_path"]).suffix.lower()
                dest = artist_dir / f"{new_id}{ext}"
                if pathlib.Path(s["audio_path"]).resolve() != dest.resolve():
                    shutil.copy2(s["audio_path"], dest)
                stored_path = str(dest)

                conn.execute(
                    "INSERT INTO songs "
                    "(id, title, artist, year, album, genre, audio_path) "
                    "VALUES (?,?,?,?,?,?,?)",
                    (new_id, s["title"], s["artist"], s["year"],
                     s["album"], s["genre"], stored_path)
                )

                lrc_text = None
                try:
                    lrc_text = syncedlyrics.search(f"{s['artist']} {s['title']}")
                except Exception:
                    pass

                utt_count = 0
                if lrc_text:
                    LRC_DIR.mkdir(parents=True, exist_ok=True)
                    (LRC_DIR / f"{new_id}.lrc").write_text(lrc_text, encoding="utf-8")
                    lines = self._parse_lrc(lrc_text)
                    if lines:
                        if nlp is None:
                            self.progress.emit("GiNZA モデルを読み込み中…")
                            nlp = spacy.load("ja_ginza")

                        # Batch-load all corrections for this song (1 query, not N)
                        # Key: normalized text → (old_utterance_id, tokens_json)
                        def _norm(t: str) -> str:
                            import unicodedata
                            return unicodedata.normalize("NFKC", t).strip()

                        corr_rows = conn.execute(
                            "SELECT utterance_id, tokens_json, text FROM token_corrections "
                            "WHERE song_artist=? AND song_title=?",
                            (s["artist"], s["title"])
                        ).fetchall()
                        # Build two maps: exact and normalized
                        corr_exact = {text: (uid, tj) for uid, tj, text in corr_rows}
                        corr_norm  = {_norm(text): (uid, tj) for uid, tj, text in corr_rows}
                        reapplied = 0

                        for line_idx, line in enumerate(lines):
                            doc = nlp(line["text"])
                            tokens = [
                                {"text": t.text, "lemma": t.lemma_, "pos": t.pos_,
                                 "dep": t.dep_, "head": t.head.text}
                                for t in doc
                            ]
                            cur = conn.execute(
                                "INSERT INTO utterances (song_id, line_idx, time_sec, text) "
                                "VALUES (?,?,?,?)",
                                (new_id, line_idx, line["time_sec"], line["text"])
                            )
                            utt_id = cur.lastrowid
                            conn.executemany(
                                "INSERT INTO tokens "
                                "(utterance_id, token_idx, surface, lemma, pos, dep, head) "
                                "VALUES (?,?,?,?,?,?,?)",
                                [(utt_id, i, t["text"], t["lemma"], t["pos"], t["dep"], t["head"])
                                 for i, t in enumerate(tokens)]
                            )
                            conn.execute(
                                "INSERT INTO utterances_fts(rowid, text) VALUES (?,?)",
                                (utt_id, line["text"])
                            )
                            # Re-apply orphaned correction: exact match first, then normalized
                            match = (corr_exact.get(line["text"])
                                     or corr_norm.get(_norm(line["text"])))
                            if match:
                                old_uid, tokens_json = match
                                if old_uid != utt_id:
                                    corrected = json.loads(tokens_json)
                                    conn.execute(
                                        "DELETE FROM tokens WHERE utterance_id=?", (utt_id,))
                                    conn.executemany(
                                        "INSERT INTO tokens"
                                        "(utterance_id,token_idx,surface,lemma,pos)"
                                        " VALUES (?,?,?,?,?)",
                                        [(utt_id, i, t['surface'],
                                          t.get('lemma', t['surface']), t.get('pos', 'NOUN'))
                                         for i, t in enumerate(corrected)])
                                    conn.execute(
                                        "UPDATE token_corrections SET utterance_id=?, text=? "
                                        "WHERE utterance_id=?",
                                        (utt_id, line["text"], old_uid))
                                    reapplied += 1
                            utt_count += 1

                        if reapplied:
                            self.progress.emit(
                                f"  └ 分词校正已恢复：{reapplied} 行")

                with open(CSV_PATH, "a", encoding="utf-8-sig", newline="") as f:
                    csv.writer(f).writerow(
                        [new_id, s["title"], s["artist"], s["year"],
                         s["album"], s["genre"], stored_path]
                    )

                lrc_msg = f"{utt_count}行" if utt_count else "LRCなし"
                added.append(f"[{new_id}] {s['artist']}《{s['title']}》({lrc_msg})")

            conn.commit()
            conn.close()

            msg = f"追加完了：{len(added)} 曲"
            if added:
                msg += "  " + "／".join(added)
            if skipped:
                msg += f"\n⚠ スキップ（重複）：" + "、".join(skipped)
            self.finished.emit(True, msg)
        except Exception as e:
            self.finished.emit(False, f"エラー：{e}")

    def _parse_lrc(self, text: str) -> list:
        results = []
        for raw_line in text.splitlines():
            m = LRC_LINE_RE.match(raw_line.strip())
            if not m:
                continue
            minutes, seconds_str, lyric = m.group(1), m.group(2), m.group(3).strip()
            if not lyric or any(kw in lyric for kw in SKIP_KEYWORDS):
                continue
            time_sec = int(minutes) * 60 + float(seconds_str)
            results.append({"time_sec": round(time_sec, 3), "text": lyric})
        return results


# ------------------------------------------------------------------ Dialog

class AddSongDialog(QDialog):
    organizeRequested = pyqtSignal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self.lang = load_settings().get("ui_language", "zh")
        self.setWindowTitle(ui_text("import_title", self.lang))
        self.setMinimumWidth(760)
        self.setMinimumHeight(560)
        self._songs: list[dict] = []

        layout = QVBoxLayout(self)
        layout.setContentsMargins(20, 18, 20, 18)
        layout.setSpacing(14)

        header = QFrame()
        header.setObjectName("ImportHeader")
        header_layout = QVBoxLayout(header)
        header_layout.setContentsMargins(18, 16, 18, 16)
        header_layout.setSpacing(4)
        title = QLabel(ui_text("import_title", self.lang))
        title.setObjectName("ImportTitle")
        subtitle = QLabel(ui_text("import_subtitle", self.lang))
        subtitle.setObjectName("ImportMuted")
        subtitle.setWordWrap(True)
        header_layout.addWidget(title)
        header_layout.addWidget(subtitle)
        layout.addWidget(header)

        pick_panel = QFrame()
        pick_panel.setObjectName("ImportPanel")
        pick_layout = QHBoxLayout(pick_panel)
        pick_layout.setContentsMargins(18, 16, 18, 16)
        pick_layout.setSpacing(14)
        pick_copy = QVBoxLayout()
        pick_title = QLabel(ui_text("import_empty", self.lang))
        pick_title.setObjectName("ImportSection")
        pick_hint = QLabel(ui_text("import_drop_hint", self.lang))
        pick_hint.setObjectName("ImportMuted")
        pick_copy.addWidget(pick_title)
        pick_copy.addWidget(pick_hint)
        pick_layout.addLayout(pick_copy, 1)
        btn_pick = QPushButton(ui_text("import_pick", self.lang))
        btn_pick.setObjectName("ImportPickButton")
        set_button_role(btn_pick, "primary")
        btn_pick.clicked.connect(self._pick_audio)
        pick_layout.addWidget(btn_pick)
        layout.addWidget(pick_panel)

        self.file_stack = QStackedWidget()
        self.empty_files_panel = QFrame()
        self.empty_files_panel.setObjectName("ImportEmptyPanel")
        empty_layout = QVBoxLayout(self.empty_files_panel)
        empty_layout.setAlignment(Qt.AlignmentFlag.AlignCenter)
        empty_title = QLabel(ui_text("import_empty", self.lang))
        empty_title.setObjectName("ImportSection")
        empty_title.setAlignment(Qt.AlignmentFlag.AlignCenter)
        empty_hint = QLabel(ui_text("import_drop_hint", self.lang))
        empty_hint.setObjectName("ImportMuted")
        empty_hint.setAlignment(Qt.AlignmentFlag.AlignCenter)
        empty_layout.addWidget(empty_title)
        empty_layout.addWidget(empty_hint)
        self.list_widget = QListWidget()
        self.list_widget.setObjectName("ImportList")
        self.list_widget.setAlternatingRowColors(True)
        self.file_stack.addWidget(self.empty_files_panel)
        self.file_stack.addWidget(self.list_widget)
        layout.addWidget(self.file_stack, 1)

        self.lbl_hint = QLabel(ui_text("import_empty", self.lang))
        self.lbl_hint.setObjectName("ImportMuted")

        btns = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel
        )
        self._ok_btn = btns.button(QDialogButtonBox.StandardButton.Ok)
        self._ok_btn.setText(ui_text("import_start", self.lang))
        set_button_role(self._ok_btn, "primary")
        cancel_btn = btns.button(QDialogButtonBox.StandardButton.Cancel)
        cancel_btn.setText(ui_text("cancel", self.lang))
        set_button_role(cancel_btn, "subtle")
        self._ok_btn.setEnabled(False)
        btns.accepted.connect(self.accept)
        btns.rejected.connect(self.reject)
        footer = QFrame()
        footer.setObjectName("ImportFooter")
        footer_layout = QHBoxLayout(footer)
        footer_layout.setContentsMargins(16, 12, 16, 12)
        footer_layout.addWidget(self.lbl_hint, 1)
        self.btn_reorg_import = QPushButton(ui_text("audio_reorg", self.lang))
        self.btn_reorg_import.setToolTip(ui_text("audio_reorg_hint", self.lang))
        set_button_role(self.btn_reorg_import, "subtle")
        self.btn_reorg_import.clicked.connect(self.organizeRequested.emit)
        footer_layout.addWidget(self.btn_reorg_import)
        footer_layout.addWidget(btns)
        layout.addWidget(footer)
        self._apply_local_style()

    def _apply_local_style(self, theme: str | None = None):
        theme = theme or load_settings().get("theme", "dark")
        if theme == "dark":
            bg = "#171a1f"; panel = "#22262d"; panel2 = "#252a32"; border = "#343c48"
            fg = "#eef3f8"; muted = "#9aa7b4"; accent = "#67e8f9"; disabled = "#313844"
        else:
            bg = "#f5f7fb"; panel = "#ffffff"; panel2 = "#ffffff"; border = "#d8e0ea"
            fg = "#1f2937"; muted = "#64748b"; accent = "#2f7de1"; disabled = "#eef3f8"
        self.setStyleSheet(f"""
            QDialog {{
                background: {bg};
                color: {fg};
            }}
            QFrame#ImportHeader, QFrame#ImportPanel, QFrame#ImportFooter, QFrame#ImportEmptyPanel {{
                background: {panel};
                border: 1px solid {border};
                border-radius: 10px;
            }}
            QLabel#ImportTitle {{
                color: {fg};
                font-size: 22px;
                font-weight: 800;
            }}
            QLabel#ImportSection {{
                color: {fg};
                font-size: 15px;
                font-weight: 700;
            }}
            QLabel#ImportMuted {{
                color: {muted};
                font-size: 13px;
            }}
            QListWidget#ImportList {{
                background: {panel};
                alternate-background-color: {panel2};
                border: 1px solid {border};
                border-radius: 10px;
                padding: 8px;
                color: {fg};
                font-size: 14px;
            }}
            QListWidget#ImportList::item {{
                min-height: 38px;
                padding: 6px 10px;
                border-radius: 7px;
            }}
            QListWidget#ImportList::item:selected {{
                background: rgba(59, 130, 246, 0.24);
            }}
            QPushButton#ImportPickButton {{
                min-width: 148px;
                min-height: 34px;
                font-weight: 700;
            }}
            QPushButton:disabled {{
                background: {disabled};
                border-color: {border};
                color: {muted};
            }}
        """)

    def refresh_theme(self, theme: str):
        self._apply_local_style(theme)

    def _pick_audio(self):
        paths, _ = QFileDialog.getOpenFileNames(
            self, ui_text("import_audio_title", self.lang), "",
            ui_text("import_audio_filter", self.lang)
        )
        if not paths:
            return
        self._songs = [self._read_tags(p) for p in paths]
        self.list_widget.clear()
        self.file_stack.setCurrentWidget(self.list_widget)
        for s in self._songs:
            ok = bool(s["title"] and s["artist"])
            mark = "OK" if ok else "!"
            name = pathlib.Path(s["audio_path"]).name
            detail = f"  [{s['artist']} / {s['title']}]" if ok else ""
            self.list_widget.addItem(f"{mark}  {name}{detail}")
        valid = sum(1 for s in self._songs if s["title"] and s["artist"])
        self._ok_btn.setEnabled(valid > 0)
        text = ui_text("import_ready", self.lang).format(valid=valid, total=len(paths))
        if valid < len(paths):
            text += "  " + ui_text("import_skip_hint", self.lang)
        self.lbl_hint.setText(text)

    def _read_tags(self, path: str) -> dict:
        data = {"title": "", "artist": "", "year": "", "album": "", "genre": "", "audio_path": path}
        try:
            import mutagen
            audio = mutagen.File(path, easy=True)
            if audio:
                def tag(k):
                    v = audio.get(k); return v[0] if v else ""
                data["title"]  = tag("title")
                data["artist"] = tag("artist")
                data["year"]   = tag("date")[:4] if tag("date") else ""
                data["album"]  = tag("album")
                data["genre"]  = tag("genre")
        except Exception:
            pass
        return data

    def get_data(self) -> list[dict]:
        return [s for s in self._songs if s["title"] and s["artist"]]


# ------------------------------------------------------------------ WordCloudDialog

_WC_FONT = r"C:\Windows\Fonts\YuGothM.ttc"

_WC_FONT_CANDIDATES = [
    ("LXGW WenKai",        str(app_dir() / "assets" / "fonts" / "LXGWWenKai-Regular.ttf")),
    ("Klee One",           str(app_dir() / "assets" / "fonts" / "KleeOne-SemiBold.ttf")),
    ("Yu Gothic Medium",    r"C:\Windows\Fonts\YuGothM.ttc"),
    ("Yu Gothic Bold",      r"C:\Windows\Fonts\YuGothB.ttc"),
    ("Yu Gothic Light",     r"C:\Windows\Fonts\YuGothL.ttc"),
    ("Meiryo",              r"C:\Windows\Fonts\meiryo.ttc"),
    ("Meiryo Bold",         r"C:\Windows\Fonts\meiryob.ttc"),
    ("MS Gothic",           r"C:\Windows\Fonts\msgothic.ttc"),
    ("BIZ UD Gothic",       r"C:\Windows\Fonts\BIZ-UDGothicR.ttc"),
    ("BIZ UD Gothic Bold",  r"C:\Windows\Fonts\BIZ-UDGothicB.ttc"),
]
_AVAIL_WC_FONTS = [(n, p) for n, p in _WC_FONT_CANDIDATES if pathlib.Path(p).exists()]

class WordCloudDialog(QDialog):
    def __init__(self, word_freq: dict, wc_font_path: str, theme: str, parent=None):
        super().__init__(parent)
        self.setWindowTitle("词云図")
        self.resize(900, 660)
        self._pixmap = None

        layout = QVBoxLayout(self)
        layout.setContentsMargins(8, 8, 8, 8)
        layout.setSpacing(6)

        self._label = QLabel()
        self._label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(self._label, stretch=1)

        btn_bar = QHBoxLayout()
        btn_bar.addStretch()
        self._btn_save = QPushButton("PNG 保存")
        self._btn_save.setEnabled(False)
        self._btn_save.clicked.connect(self._save)
        btn_bar.addWidget(self._btn_save)
        layout.addLayout(btn_bar)

        self._generate(word_freq, wc_font_path, theme)

    def _generate(self, word_freq: dict, wc_font_path: str, theme: str):
        try:
            from wordcloud import WordCloud
        except ImportError:
            self._label.setText("wordcloud ライブラリが未インストールです。\nvenv で: pip install wordcloud")
            return
        from io import BytesIO
        from PyQt6.QtWidgets import QApplication
        from PyQt6.QtGui import QPixmap

        font_path = wc_font_path if pathlib.Path(wc_font_path).exists() else None

        # 按屏幕 DPR 生成物理像素分辨率，避免高 DPI 模糊
        dpr = QApplication.instance().devicePixelRatio()
        W, H = int(880 * dpr), int(600 * dpr)

        wc_bg = _THEME_COLORS[theme]["wc_bg"]
        wc = WordCloud(
            font_path=font_path,
            width=W, height=H,
            background_color=wc_bg,
            colormap="tab20",
            max_words=200,
            prefer_horizontal=1.0,
            min_font_size=12,
            max_font_size=int(200 * dpr),
        ).generate_from_frequencies(word_freq)

        buf = BytesIO()
        wc.to_image().save(buf, "PNG")
        buf.seek(0)
        pm = QPixmap()
        pm.loadFromData(buf.read())
        pm.setDevicePixelRatio(dpr)   # 告知 Qt 这是高 DPI 图
        self._pixmap = pm
        self._label.setPixmap(pm)
        self._btn_save.setEnabled(True)

    def _save(self):
        path, _ = QFileDialog.getSaveFileName(self, "PNG 保存", "wordcloud.png", "PNG (*.png)")
        if path and self._pixmap:
            self._pixmap.save(path)


# ------------------------------------------------------------------ MainWindow

class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()

        self._cfg = load_settings()
        self.resize(self._cfg["window_width"], self._cfg["window_height"])

        self._results: list[dict] = []
        self._stats_data: list = []
        self._stats_row_index: dict = {}
        self._missing_audio: list = []
        self.lang = self._cfg.get("ui_language", "zh")
        self._current_route = "search"
        self.setWindowTitle(ui_text("app_title", self.lang))
        self._audio_output = QAudioOutput(self)
        self._audio_output.setVolume(1.0)
        self._player = QMediaPlayer(self)
        self._player.setAudioOutput(self._audio_output)
        self._player.errorOccurred.connect(self._on_player_error)
        self._playback_rate = normalize_playback_rate(self._cfg.get("playback_rate", 1.0))
        self._player.setPlaybackRate(self._playback_rate)
        self._loop_start_ms = 0
        self._loop_end_ms   = 0
        self._loop_timer = QTimer()
        self._loop_timer.setInterval(150)
        self._loop_timer.timeout.connect(self._check_loop)

        self._build_ui()
        self._apply_saved_settings()
        self._load_artists()
        self._check_missing_audio()

    # ------------------------------------------------------------------ UI
    def _build_ui(self):
        shell = QWidget()
        shell_layout = QHBoxLayout(shell)
        shell_layout.setContentsMargins(0, 0, 0, 0)
        shell_layout.setSpacing(0)

        self.navigation = NavigationInterface(
            self, showMenuButton=True, showReturnButton=False, collapsible=True
        )
        self.navigation.setExpandWidth(218)
        self.navigation.setMinimumExpandWidth(168)
        shell_layout.addWidget(self.navigation)

        self.stack = QStackedWidget()
        self.tabs = self.stack  # compatibility for existing handlers
        self.search_page = self._build_search_tab()
        self.stats_page = self._build_stats_tab()
        self.report_page = self._build_report_tab()
        self.songs_page = self._build_embedded_tool_page()
        self.import_page = self._build_embedded_tool_page()
        self.anki_page = self._build_embedded_tool_page()
        self.dict_page = self._build_embedded_tool_page()
        self.settings_page = self._build_embedded_tool_page()
        self._embedded_tool_widgets: dict[str, QWidget] = {}
        self.stack.addWidget(self.search_page)
        self.stack.addWidget(self.stats_page)
        self.stack.addWidget(self.report_page)
        self.stack.addWidget(self.songs_page)
        self.stack.addWidget(self.import_page)
        self.stack.addWidget(self.anki_page)
        self.stack.addWidget(self.dict_page)
        self.stack.addWidget(self.settings_page)
        shell_layout.addWidget(self.stack, 1)

        self.setCentralWidget(shell)
        self._build_navigation()
        self.setStatusBar(QStatusBar())
        self.statusBar().showMessage(ui_text("ready", self.lang))
        self._setup_shortcuts()

    def _rebuild_ui_after_settings(self, route_key: str = "search"):
        self.lang = self._cfg.get("ui_language", "zh")
        self.setWindowTitle(ui_text("app_title", self.lang))
        self._build_ui()
        self._apply_saved_settings()
        self._load_artists()
        if self._results:
            self._on_results(self._results)
        if self._stats_data:
            self._on_stats_results(self._stats_data)
        self._show_page(route_key if route_key in self._route_indexes else "search")

    def _build_navigation(self):
        self._route_indexes = {
            "search": 0,
            "stats": 1,
            "report": 2,
            "songs": 3,
            "import": 4,
            "anki": 5,
            "dict": 6,
            "settings": 7,
        }
        self.navigation.addItem(
            "search", FluentIcon.SEARCH, ui_text("nav_search", self.lang),
            onClick=lambda: self._show_page("search"),
            tooltip=ui_text("tooltip_search", self.lang),
        )
        self.navigation.addItem(
            "stats", FluentIcon.PIE_SINGLE, ui_text("nav_stats", self.lang),
            onClick=lambda: self._show_page("stats"),
            tooltip=ui_text("tooltip_stats", self.lang),
        )
        self.navigation.addItem(
            "report", FluentIcon.DOCUMENT, ui_text("nav_report", self.lang),
            onClick=lambda: self._show_page("report"),
            tooltip=ui_text("tooltip_report", self.lang),
        )
        self.navigation.addSeparator()
        self.navigation.addItem(
            "songs", FluentIcon.MUSIC_FOLDER, ui_text("nav_songs", self.lang),
            onClick=lambda: self._show_page("songs"),
            tooltip=ui_text("tooltip_songs", self.lang),
        )
        self.navigation.addItem(
            "import", FluentIcon.ADD_TO, ui_text("nav_import", self.lang),
            onClick=lambda: self._show_page("import"),
            tooltip=ui_text("tooltip_import", self.lang),
        )
        self.navigation.addItem(
            "anki", FluentIcon.SEND, "Anki",
            onClick=lambda: self._show_page("anki"),
            position=NavigationItemPosition.BOTTOM,
            tooltip=ui_text("tooltip_anki", self.lang),
        )
        self.navigation.addItem(
            "dict", FluentIcon.DICTIONARY, ui_text("nav_dict", self.lang),
            onClick=lambda: self._show_page("dict"),
            position=NavigationItemPosition.BOTTOM,
            tooltip=ui_text("tooltip_dict", self.lang),
        )
        self.navigation.addItem(
            "settings", FluentIcon.SETTING, ui_text("nav_settings", self.lang),
            onClick=lambda: self._show_page("settings"),
            position=NavigationItemPosition.BOTTOM,
            tooltip=ui_text("tooltip_settings", self.lang),
        )
        self.navigation.setCurrentItem("search")

    def _show_page(self, route_key: str):
        idx = self._route_indexes.get(route_key)
        if idx is None:
            return
        self._ensure_embedded_tool(route_key)
        if route_key == "search":
            self._refresh_playback_rate_from_settings()
        elif route_key == "songs":
            widget = self._embedded_tool_widgets.get("songs")
            if widget is not None and hasattr(widget, "refresh_playback_rate_from_settings"):
                widget.refresh_playback_rate_from_settings()
        self.stack.setCurrentIndex(idx)
        self._current_route = route_key
        fade_in_widget(self.stack.currentWidget(), 120)
        self.navigation.setCurrentItem(route_key)

    def _build_embedded_tool_page(self) -> QWidget:
        page = QWidget()
        layout = QVBoxLayout(page)
        layout.setContentsMargins(14, 12, 14, 10)
        layout.setSpacing(0)
        return page

    def _embed_dialog_as_page(self, route_key: str, dialog: QDialog, page: QWidget, *, close_to_search: bool = True):
        from PyQt6.QtWidgets import QSizePolicy as QSP
        dialog.setParent(page)
        dialog.setWindowFlags(Qt.WindowType.Widget)
        dialog.setSizePolicy(QSP.Policy.Expanding, QSP.Policy.Expanding)
        if close_to_search:
            dialog.finished.connect(lambda _code, key=route_key: self._on_embedded_tool_closed(key))
        page.layout().addWidget(dialog)
        self._embedded_tool_widgets[route_key] = dialog
        dialog.show()

    def _ensure_embedded_tool(self, route_key: str):
        existing = self._embedded_tool_widgets.get(route_key)
        if existing is not None:
            existing.show()
            return
        if route_key == "songs":
            from dialogs.song_manager import SongManagerDialog
            self._embed_dialog_as_page(route_key, SongManagerDialog(self.songs_page), self.songs_page)
        elif route_key == "import":
            dlg = AddSongDialog(self.import_page)
            dlg.accepted.connect(lambda d=dlg: self._start_add_from_dialog(d))
            dlg.organizeRequested.connect(self.reorganize_audio)
            self._embed_dialog_as_page(route_key, dlg, self.import_page)
        elif route_key == "anki":
            from dialogs.anki_export import AnkiExportDialog
            self._embed_dialog_as_page(route_key, AnkiExportDialog(self.anki_page), self.anki_page)
        elif route_key == "dict":
            from dialogs.dict_manager import DictManagerDialog
            self._embed_dialog_as_page(route_key, DictManagerDialog(self.dict_page), self.dict_page)
        elif route_key == "settings":
            dlg = FontSettingsDialog(
                self._cfg.get("font_family", "Microsoft YaHei UI"),
                self._cfg.get("font_size", 11),
                self._cfg.get("wc_font_path", _WC_FONT),
                self._cfg.get("theme", "dark"),
                self._cfg.get("yomitan_zh_dict", ""),
                self._cfg.get("ui_language", "zh"),
                self._cfg.get("keyword_color", ""),
                self._cfg.get("custom_font_paths", []),
                self.settings_page,
            )
            dlg._original_theme = self._cfg.get("theme", "dark")
            dlg.themePreviewRequested.connect(self._preview_theme)
            dlg.accepted.connect(lambda d=dlg: self._apply_font_settings_dialog(d))
            dlg.rejected.connect(lambda d=dlg: self._cancel_theme_preview(d))
            self._embed_dialog_as_page(route_key, dlg, self.settings_page, close_to_search=False)

    def _on_embedded_tool_closed(self, route_key: str):
        widget = self._embedded_tool_widgets.get(route_key)
        if widget is not None:
            widget.hide()
        if getattr(self, "_current_route", "") == route_key:
            self._show_page("search")

    def _setup_shortcuts(self):
        for shortcut in getattr(self, "_shortcuts", []):
            shortcut.deleteLater()
        self._shortcuts = []
        # Ctrl+F：聚焦搜索框并全选
        sc = QShortcut(QKeySequence("Ctrl+F"), self)
        sc.activated.connect(self._focus_search)
        self._shortcuts.append(sc)
        # Ctrl+E：导出 CSV
        sc = QShortcut(QKeySequence("Ctrl+E"), self)
        sc.activated.connect(self.export_csv)
        self._shortcuts.append(sc)
        # Ctrl+L：切换单行循环
        sc = QShortcut(QKeySequence("Ctrl+L"), self)
        sc.activated.connect(lambda: self.chk_loop.setChecked(not self.chk_loop.isChecked()))
        self._shortcuts.append(sc)
        # 结果表：Enter 播放 / Space 暂停恢复 / Escape 返回搜索框
        self.table.installEventFilter(self)

    def eventFilter(self, obj, event):
        if obj is self.table and event.type() == QEvent.Type.KeyPress:
            key = event.key()
            if key in (Qt.Key.Key_Return, Qt.Key.Key_Enter):
                rows = self.table.selectedIndexes()
                if rows:
                    self.on_double_click(rows[0].row(), 0)
                return True
            if key == Qt.Key.Key_Space:
                self.toggle_play_pause()
                return True
            if key == Qt.Key.Key_Escape:
                self._focus_search()
                return True
        return super().eventFilter(obj, event)

    def _focus_search(self):
        self._show_page("search")
        self.input.setFocus()
        self.input.selectAll()

    def _build_menubar(self):
        mb = self.menuBar()
        mgmt = mb.addMenu("🎵 曲管理")
        mgmt.addAction("曲一覧・編集・削除", self.open_song_manager)
        mgmt.addSeparator()
        mgmt.addAction("＋ 曲を追加",  self.add_song)
        mgmt.addAction("🗂 音声整理",  self.reorganize_audio)
        mgmt.addAction("⚠ 音声修復",  self.repair_audio)
        cfg = mb.addMenu("⚙ 設定")
        cfg.addAction("フォント設定…", self.open_font_settings)
        cfg.addAction("📖 管理词典…", self.open_dict_manager)
        anki_m = mb.addMenu("📤 Anki")
        anki_m.addAction("Anki 導出…", self.open_anki_export)
        report_m = mb.addMenu("📊 报告")
        report_m.addAction("生成挖词报告…", self._generate_report)

    def _build_search_tab(self) -> QWidget:
        root = QWidget()
        layout = QVBoxLayout(root)
        layout.setSpacing(10)
        layout.setContentsMargins(14, 12, 14, 8)

        # 搜索栏
        search_panel = QFrame()
        search_panel.setObjectName("TopPanel")
        bar = QHBoxLayout(search_panel)
        bar.setContentsMargins(12, 10, 12, 10)
        bar.setSpacing(10)
        self.input = QLineEdit()
        self.input.setObjectName("SearchInput")
        self.input.setPlaceholderText(ui_text("search_placeholder", self.lang))
        self.input.setFont(QFont(self._cfg.get("font_family", "Microsoft YaHei UI"), self._cfg.get("font_size", 11) + 1))
        self.input.returnPressed.connect(self.do_search)
        bar.addWidget(self.input)

        self.rb_surface = QRadioButton(ui_text("surface", self.lang))
        self.rb_lemma   = QRadioButton(ui_text("lemma", self.lang))
        self.rb_surface.setChecked(True)
        grp = QButtonGroup(self)
        grp.addButton(self.rb_surface)
        grp.addButton(self.rb_lemma)
        bar.addWidget(self.rb_surface)
        bar.addWidget(self.rb_lemma)

        self.btn_search = QPushButton(ui_text("search_button", self.lang))
        self.btn_search.setFont(QFont("", self._cfg.get("font_size", 11)))
        set_button_role(self.btn_search, "primary")
        self.btn_search.clicked.connect(self.do_search)
        bar.addWidget(self.btn_search)
        layout.addWidget(search_panel)

        # ── 筛选条件面板（可折叠）──────────────────────────────
        toggle_row = QHBoxLayout()
        self.btn_filter_toggle = QPushButton(ui_text("filter_open", self.lang))
        set_button_role(self.btn_filter_toggle, "subtle")
        self.btn_filter_toggle.clicked.connect(self._toggle_filter_panel)
        toggle_row.addWidget(self.btn_filter_toggle)
        toggle_row.addStretch()
        layout.addLayout(toggle_row)

        self.filter_panel = QFrame()
        self.filter_panel.setObjectName("FilterPanel")
        from PyQt6.QtWidgets import QGridLayout
        fp = QGridLayout(self.filter_panel)
        fp.setContentsMargins(10, 8, 10, 8)
        fp.setHorizontalSpacing(10)
        fp.setVerticalSpacing(8)
        # 列宽比例：label(固定) | 控件(弹性) | label(固定) | 控件(弹性)
        fp.setColumnMinimumWidth(0, 38)
        fp.setColumnMinimumWidth(2, 38)
        fp.setColumnStretch(1, 5)   # 歌手下拉占更多空间
        fp.setColumnStretch(3, 3)   # 品詞下拉

        fp.addWidget(QLabel(ui_text("artist", self.lang)), 0, 0)
        self.artist_widget = MultiSelectComboBox(ui_text("all_artists", self.lang), lang=self.lang)
        fp.addWidget(self.artist_widget, 0, 1)
        fp.addWidget(QLabel(ui_text("pos", self.lang)), 0, 2)
        self.pos_combo = QComboBox()
        for label, value in pos_options(self.lang):
            self.pos_combo.addItem(label, value)
        fp.addWidget(self.pos_combo, 0, 3)

        fp.addWidget(QLabel(ui_text("context", self.lang)), 1, 0)
        self.ctx_len_spin = QSpinBox()
        self.ctx_len_spin.setRange(5, 200)
        self.ctx_len_spin.setValue(40)
        self.ctx_len_spin.setSuffix(" 字")
        self.ctx_len_spin.setFixedWidth(112)
        fp.addWidget(self.ctx_len_spin, 1, 1, Qt.AlignmentFlag.AlignLeft)
        chk_widget = QWidget()
        chk_lay = QHBoxLayout(chk_widget)
        chk_lay.setContentsMargins(0, 0, 0, 0)
        chk_lay.setSpacing(20)
        self.chk_cross_line = QCheckBox(ui_text("cross_line", self.lang))
        self.chk_jp_only    = QCheckBox(ui_text("jp_only", self.lang))
        self.chk_dedup      = QCheckBox(ui_text("dedup", self.lang))
        self.chk_dedup.setChecked(True)
        self.chk_dedup.setToolTip("同曲中出现多次的相同歌词行折叠为一行，时刻列显示重复次数")
        chk_lay.addWidget(self.chk_cross_line)
        chk_lay.addWidget(self.chk_jp_only)
        chk_lay.addWidget(self.chk_dedup)
        chk_lay.addStretch()
        fp.addWidget(chk_widget, 1, 2, 1, 2)   # 跨品詞列

        layout.addWidget(self.filter_panel)

        # 结果计数 + 操作按钮
        action_panel = QFrame()
        action_panel.setObjectName("ActionPanel")
        count_bar = QHBoxLayout(action_panel)
        count_bar.setContentsMargins(10, 8, 10, 8)
        count_bar.setSpacing(8)
        self.lbl_count = QLabel(ui_text("results", self.lang))
        count_bar.addWidget(self.lbl_count)
        count_bar.addStretch()
        self.btn_repair = QPushButton(ui_text("audio_repair", self.lang))
        set_button_role(self.btn_repair, "danger")
        self.btn_repair.setToolTip("音声ファイルが見つからない曲を修復します")
        self.btn_repair.clicked.connect(self.repair_audio)
        self.btn_repair.hide()
        count_bar.addWidget(self.btn_repair)
        self.btn_reorg = QPushButton(ui_text("audio_reorg", self.lang))
        set_button_role(self.btn_reorg, "subtle")
        self.btn_reorg.setToolTip(ui_text("audio_reorg_hint", self.lang))
        self.btn_reorg.clicked.connect(self.reorganize_audio)
        count_bar.addWidget(self.btn_reorg)
        self.btn_add_song = QPushButton(ui_text("add_song", self.lang))
        set_button_role(self.btn_add_song, "subtle")
        self.btn_add_song.clicked.connect(self.add_song)
        count_bar.addWidget(self.btn_add_song)
        self.btn_export = QPushButton(ui_text("export_csv", self.lang))
        set_button_role(self.btn_export, "subtle")
        self.btn_export.setEnabled(False)
        self.btn_export.clicked.connect(self.export_csv)
        count_bar.addWidget(self.btn_export)
        layout.addWidget(action_panel)

        # KWIC 表格
        self.table = QTableWidget()
        self.table.setColumnCount(7)
        self.table.setHorizontalHeaderLabels([
            ui_text("table_play", self.lang),
            ui_text("table_artist", self.lang),
            ui_text("table_song", self.lang),
            ui_text("table_time", self.lang),
            ui_text("table_left", self.lang),
            ui_text("table_keyword", self.lang),
            ui_text("table_right", self.lang),
        ])
        hh = self.table.horizontalHeader()
        hh.setSectionResizeMode(QHeaderView.ResizeMode.Interactive)
        hh.setSectionResizeMode(0, QHeaderView.ResizeMode.Fixed)
        hh.setStretchLastSection(True)
        hh.setMinimumSectionSize(36)
        self.table.setColumnWidth(0, 36)
        # 默认列宽（可通过拖拽调整，窗口关闭时保存）
        _DEFAULT_COL_WIDTHS = [110, 170, 78, 260, 90]
        for i, w in enumerate(_DEFAULT_COL_WIDTHS):
            self.table.setColumnWidth(i + 1, w)
        hh.sectionResized.connect(self._save_col_widths)
        self.table.setSelectionBehavior(QTableWidget.SelectionBehavior.SelectRows)
        self.table.setEditTriggers(QTableWidget.EditTrigger.NoEditTriggers)
        self.table.setAlternatingRowColors(True)
        self.table.verticalHeader().setDefaultSectionSize(40)
        self.table.cellDoubleClicked.connect(self.on_double_click)
        self.table.setContextMenuPolicy(Qt.ContextMenuPolicy.CustomContextMenu)
        self.table.customContextMenuRequested.connect(self._on_kwic_context_menu)
        tune_table(self.table, 38)
        layout.addWidget(self.table)

        # 播放器栏
        player_panel = QFrame()
        player_panel.setObjectName("ActionPanel")
        player_bar = QHBoxLayout(player_panel)
        player_bar.setContentsMargins(10, 7, 10, 7)
        player_bar.setSpacing(8)
        self.lbl_now = QLabel("▷ —")
        self.lbl_now.setFont(QFont("", 11))
        player_bar.addWidget(self.lbl_now, stretch=1)
        player_bar.addWidget(QLabel(ui_text("playback_speed", self.lang)))
        self.speed_control = PlaybackSpeedControl(self._playback_rate, self)
        self.speed_control.rateChanged.connect(self._set_playback_rate)
        player_bar.addWidget(self.speed_control)
        self.btn_play = QPushButton("▶")
        self.btn_play.setFixedWidth(36)
        set_button_role(self.btn_play, "primary")
        self.btn_play.clicked.connect(self.toggle_play_pause)
        player_bar.addWidget(self.btn_play)
        self.chk_loop = QCheckBox(ui_text("loop", self.lang))
        self.chk_loop.toggled.connect(self._on_loop_toggled)
        player_bar.addWidget(self.chk_loop)
        layout.addWidget(player_panel)

        return root

    def _build_stats_tab(self) -> QWidget:
        root = QWidget()
        root.setObjectName("ModernPage")
        layout = QVBoxLayout(root)
        layout.setSpacing(14)
        layout.setContentsMargins(18, 16, 18, 12)

        header = QFrame()
        header.setObjectName("ModernPanel")
        header_layout = QVBoxLayout(header)
        header_layout.setContentsMargins(18, 16, 18, 16)
        header_layout.setSpacing(4)
        title = QLabel(ui_text("stats_title", self.lang))
        title.setObjectName("PageTitle")
        subtitle = QLabel(ui_text("stats_subtitle", self.lang))
        subtitle.setObjectName("MutedLabel")
        subtitle.setWordWrap(True)
        header_layout.addWidget(title)
        header_layout.addWidget(subtitle)
        layout.addWidget(header)

        ctrl_panel = QFrame()
        ctrl_panel.setObjectName("ModernPanel")
        ctrl = QHBoxLayout(ctrl_panel)
        ctrl.setContentsMargins(16, 12, 16, 12)
        ctrl.setSpacing(12)
        section = QLabel(ui_text("stats_filters", self.lang))
        section.setObjectName("SectionTitle")
        ctrl.addWidget(section)
        self.stats_artist_widget = MultiSelectComboBox(ui_text("all_artists", self.lang), lang=self.lang)
        self.stats_artist_widget.setMinimumWidth(220)
        ctrl.addWidget(self.stats_artist_widget)
        ctrl.addSpacing(16)
        ctrl.addWidget(QLabel(ui_text("pos", self.lang)))
        self.stats_pos_combo = QComboBox()
        self.stats_pos_combo.setMinimumWidth(160)
        for label, value in pos_options(self.lang):
            self.stats_pos_combo.addItem(label, value)
        ctrl.addWidget(self.stats_pos_combo)
        self.btn_load_stats = QPushButton(ui_text("stats_load", self.lang))
        set_button_role(self.btn_load_stats, "primary")
        self.btn_load_stats.clicked.connect(self.load_stats)
        ctrl.addWidget(self.btn_load_stats)
        self.btn_wordcloud = QPushButton(ui_text("wordcloud", self.lang))
        set_button_role(self.btn_wordcloud, "subtle")
        self.btn_wordcloud.setEnabled(False)
        self.btn_wordcloud.clicked.connect(self._show_wordcloud)
        ctrl.addWidget(self.btn_wordcloud)
        ctrl.addSpacing(16)
        self.chk_stats_jp_only = QCheckBox(ui_text("jp_only", self.lang))
        ctrl.addWidget(self.chk_stats_jp_only)
        ctrl.addStretch()
        layout.addWidget(ctrl_panel)

        table_panel = QFrame()
        table_panel.setObjectName("ModernPanel")
        table_layout = QVBoxLayout(table_panel)
        table_layout.setContentsMargins(16, 14, 16, 14)
        table_layout.setSpacing(10)
        table_header = QHBoxLayout()
        table_title = QLabel(ui_text("stats_results_title", self.lang))
        table_title.setObjectName("SectionTitle")
        table_header.addWidget(table_title)
        table_header.addStretch()
        self.lbl_stats_count = QLabel(ui_text("stats_empty", self.lang))
        self.lbl_stats_count.setObjectName("MutedLabel")
        table_header.addWidget(self.lbl_stats_count)
        table_layout.addLayout(table_header)

        self.stats_table = QTableWidget()
        self.stats_table.setObjectName("ModernTable")
        self.stats_table.setColumnCount(6)
        self.stats_table.setHorizontalHeaderLabels(ui_text("stats_headers", self.lang))
        shh = self.stats_table.horizontalHeader()
        shh.setSectionResizeMode(0, QHeaderView.ResizeMode.ResizeToContents)
        shh.setSectionResizeMode(1, QHeaderView.ResizeMode.ResizeToContents)
        shh.setSectionResizeMode(2, QHeaderView.ResizeMode.ResizeToContents)
        shh.setSectionResizeMode(3, QHeaderView.ResizeMode.ResizeToContents)
        shh.setSectionResizeMode(4, QHeaderView.ResizeMode.ResizeToContents)
        shh.setSectionResizeMode(5, QHeaderView.ResizeMode.Stretch)
        self.stats_table.setSelectionBehavior(QTableWidget.SelectionBehavior.SelectRows)
        self.stats_table.setEditTriggers(QTableWidget.EditTrigger.NoEditTriggers)
        self.stats_table.setAlternatingRowColors(True)
        self.stats_table.cellDoubleClicked.connect(self._on_stats_double_click)
        tune_table(self.stats_table, 40)
        table_layout.addWidget(self.stats_table, 1)

        hint = QLabel(ui_text("stats_hint", self.lang))
        hint.setObjectName("MutedLabel")
        table_layout.addWidget(hint)
        layout.addWidget(table_panel, 1)
        self._apply_modern_page_style(root)

        return root

    def _build_report_tab(self) -> QWidget:
        root = QWidget()
        layout = QVBoxLayout(root)
        layout.setSpacing(10)
        layout.setContentsMargins(14, 12, 14, 8)

        ctrl_panel = QFrame()
        ctrl_panel.setObjectName("TopPanel")
        ctrl = QHBoxLayout(ctrl_panel)
        ctrl.setContentsMargins(12, 10, 12, 10)
        ctrl.setSpacing(10)
        self.report_artist_widget = MultiSelectComboBox(ui_text("all_artists", self.lang), lang=self.lang)
        ctrl.addWidget(self.report_artist_widget)
        ctrl.addSpacing(16)
        self.chk_report_jp_only = QCheckBox(ui_text("jp_only", self.lang))
        ctrl.addWidget(self.chk_report_jp_only)
        ctrl.addSpacing(16)
        self.btn_gen_report = QPushButton("📊  " + ui_text("report_generate", self.lang))
        set_button_role(self.btn_gen_report, "primary")
        self.btn_gen_report.clicked.connect(self.generate_report)
        ctrl.addWidget(self.btn_gen_report)
        self.btn_export_report = QPushButton(ui_text("report_export_txt", self.lang))
        set_button_role(self.btn_export_report, "subtle")
        self.btn_export_report.setEnabled(False)
        self.btn_export_report.clicked.connect(self._export_report_txt)
        ctrl.addWidget(self.btn_export_report)
        ctrl.addStretch()
        self.lbl_report_status = QLabel("")
        ctrl.addWidget(self.lbl_report_status)
        layout.addWidget(ctrl_panel)

        self.report_scroll = QScrollArea()
        self.report_scroll.setWidgetResizable(True)
        self.report_scroll.setFrameShape(QFrame.Shape.NoFrame)
        self.report_content = QWidget()
        self.report_content.setObjectName("ReportContent")
        self.report_body = QVBoxLayout(self.report_content)
        self.report_body.setContentsMargins(18, 18, 18, 18)
        self.report_body.setSpacing(14)
        self.report_scroll.setWidget(self.report_content)
        layout.addWidget(self.report_scroll)
        self._apply_report_style()
        self._render_report_empty()

        return root

    def _report_colors(self, theme: str | None = None) -> dict:
        theme = theme or getattr(self, "_preview_theme_name", None) or self._cfg.get("theme", "dark")
        if theme == "dark":
            return {
                "bg": "#171a1f",
                "panel": "#22262d",
                "panel2": "#252a32",
                "border": "#343c48",
                "fg": "#eef3f8",
                "muted": "#9aa7b4",
                "accent": "#3b82f6",
                "accent2": "#67e8f9",
                "track": "#313844",
                "table_header": "#2a3039",
            }
        return {
            "bg": "#f5f7fb",
            "panel": "#ffffff",
            "panel2": "#ffffff",
            "border": "#d8e0ea",
            "fg": "#1f2937",
            "muted": "#64748b",
            "accent": "#2f7de1",
            "accent2": "#0891b2",
            "track": "#e6edf5",
            "table_header": "#eef3f8",
        }

    def _apply_modern_page_style(self, widget: QWidget, theme: str | None = None):
        c = self._report_colors(theme)
        widget.setStyleSheet(f"""
            QWidget#ModernPage {{
                background: {c["bg"]};
                color: {c["fg"]};
            }}
            QFrame#ModernPanel {{
                background: {c["panel"]};
                border: 1px solid {c["border"]};
                border-radius: 10px;
            }}
            QLabel#PageTitle {{
                color: {c["fg"]};
                font-size: 22px;
                font-weight: 800;
            }}
            QLabel#SectionTitle {{
                color: {c["fg"]};
                font-size: 15px;
                font-weight: 700;
            }}
            QLabel#MutedLabel {{
                color: {c["muted"]};
                font-size: 13px;
            }}
            QTableWidget#ModernTable {{
                background: {c["panel"]};
                alternate-background-color: {c["panel2"]};
                border: 1px solid {c["border"]};
                border-radius: 8px;
                gridline-color: {c["border"]};
                color: {c["fg"]};
                font-size: 13px;
            }}
            QTableWidget#ModernTable::item {{
                padding: 7px 9px;
                border: 0;
            }}
            QTableWidget#ModernTable::item:selected {{
                background: rgba(59, 130, 246, 0.28);
                color: {c["fg"]};
            }}
            QHeaderView::section {{
                background: {c["table_header"]};
                color: {c["fg"]};
                border: 0;
                border-right: 1px solid {c["border"]};
                padding: 8px;
                font-weight: 700;
            }}
        """)

    def _apply_report_style(self, theme: str | None = None):
        c = self._report_colors(theme)
        self.report_content.setStyleSheet(f"""
            QWidget#ReportContent {{
                background: {c["bg"]};
                color: {c["fg"]};
            }}
            QFrame#ReportPanel, QFrame#ReportCard {{
                background: {c["panel"]};
                border: 1px solid {c["border"]};
                border-radius: 10px;
            }}
            QLabel#ReportTitle {{
                color: {c["fg"]};
                font-size: 18px;
                font-weight: 700;
            }}
            QLabel#ReportSection {{
                color: {c["fg"]};
                font-size: 16px;
                font-weight: 700;
            }}
            QLabel#ReportMuted {{
                color: {c["muted"]};
                font-size: 12px;
            }}
            QLabel#ReportKpi {{
                color: {c["accent2"]};
                font-size: 26px;
                font-weight: 800;
            }}
            QLabel#ReportLabel {{
                color: {c["fg"]};
                font-size: 12px;
                font-weight: 600;
            }}
            QProgressBar#ReportBar {{
                background: {c["track"]};
                border: 0;
                border-radius: 5px;
                height: 10px;
                text-align: center;
            }}
            QProgressBar#ReportBar::chunk {{
                background: {c["accent"]};
                border-radius: 5px;
            }}
            QTableWidget#ReportTable {{
                background: {c["panel"]};
                alternate-background-color: {c["panel2"]};
                border: 1px solid {c["border"]};
                border-radius: 8px;
                gridline-color: {c["border"]};
                color: {c["fg"]};
                font-size: 13px;
            }}
            QTableWidget#ReportTable::item {{
                padding: 6px 8px;
                border: 0;
            }}
            QHeaderView::section {{
                background: {c["table_header"]};
                color: {c["fg"]};
                border: 0;
                border-right: 1px solid {c["border"]};
                padding: 8px;
                font-weight: 700;
            }}
        """)

    def _clear_report_body(self):
        while self.report_body.count():
            item = self.report_body.takeAt(0)
            widget = item.widget()
            child_layout = item.layout()
            if widget is not None:
                widget.deleteLater()
            elif child_layout is not None:
                self._clear_layout(child_layout)

    def _clear_layout(self, layout):
        while layout.count():
            item = layout.takeAt(0)
            widget = item.widget()
            child_layout = item.layout()
            if widget is not None:
                widget.deleteLater()
            elif child_layout is not None:
                self._clear_layout(child_layout)

    def _render_report_empty(self):
        self._clear_report_body()
        card = QFrame()
        card.setObjectName("ReportPanel")
        lay = QVBoxLayout(card)
        lay.setContentsMargins(22, 22, 22, 22)
        title = QLabel(ui_text("report_empty", self.lang))
        title.setObjectName("ReportTitle")
        lay.addWidget(title)
        hint = QLabel(ui_text("ready", self.lang))
        hint.setObjectName("ReportMuted")
        lay.addWidget(hint)
        self.report_body.addWidget(card)
        self.report_body.addStretch()

    def _make_report_card(self, label: str, value: str, note: str = "") -> QFrame:
        card = QFrame()
        card.setObjectName("ReportCard")
        lay = QVBoxLayout(card)
        lay.setContentsMargins(16, 14, 16, 14)
        lay.setSpacing(4)
        value_label = QLabel(value)
        value_label.setObjectName("ReportKpi")
        value_label.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        lay.addWidget(value_label)
        label_widget = QLabel(label)
        label_widget.setObjectName("ReportLabel")
        lay.addWidget(label_widget)
        if note:
            note_widget = QLabel(note)
            note_widget.setObjectName("ReportMuted")
            note_widget.setWordWrap(True)
            lay.addWidget(note_widget)
        lay.addStretch()
        return card

    def _make_report_section(self, title: str, subtitle: str = "") -> QFrame:
        panel = QFrame()
        panel.setObjectName("ReportPanel")
        panel_layout = QVBoxLayout(panel)
        panel_layout.setContentsMargins(18, 16, 18, 18)
        panel_layout.setSpacing(12)
        header = QLabel(title)
        header.setObjectName("ReportSection")
        panel_layout.addWidget(header)
        if subtitle:
            sub = QLabel(subtitle)
            sub.setObjectName("ReportMuted")
            sub.setWordWrap(True)
            panel_layout.addWidget(sub)
        return panel

    def _make_report_bar(self, percent: float, max_percent: float = 100.0) -> QProgressBar:
        bar = QProgressBar()
        bar.setObjectName("ReportBar")
        bar.setRange(0, 1000)
        value = 0 if not max_percent else int(max(0.0, min(percent / max_percent, 1.0)) * 1000)
        bar.setValue(value)
        bar.setTextVisible(False)
        return bar

    def _make_report_table(self, headers: list[str], rows: int) -> QTableWidget:
        table = QTableWidget(rows, len(headers))
        table.setObjectName("ReportTable")
        table.setHorizontalHeaderLabels(headers)
        table.setEditTriggers(QTableWidget.EditTrigger.NoEditTriggers)
        table.setSelectionMode(QTableWidget.SelectionMode.NoSelection)
        table.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        table.setAlternatingRowColors(True)
        table.verticalHeader().setVisible(False)
        table.horizontalHeader().setHighlightSections(False)
        table.horizontalHeader().setSectionResizeMode(QHeaderView.ResizeMode.Stretch)
        table.setShowGrid(False)
        table.verticalHeader().setDefaultSectionSize(40)
        table.setMinimumHeight(54 + rows * 40)
        return table

    def _render_report_dashboard(self, report: dict):
        self._apply_report_style()
        self._clear_report_body()
        lang = self.lang
        artists = report.get("artist_filter") or []
        filter_desc = "、".join(artists) if artists else ui_text("report_all_artists", lang)
        if report.get("jp_only"):
            filter_desc += f" / {ui_text('jp_only', lang)}"

        header = QFrame()
        header.setObjectName("ReportPanel")
        h = QHBoxLayout(header)
        h.setContentsMargins(18, 14, 18, 14)
        h.setSpacing(18)
        title_box = QVBoxLayout()
        title = QLabel(ui_text("report_overview", lang))
        title.setObjectName("ReportTitle")
        title_box.addWidget(title)
        filter_label = QLabel(f"{ui_text('report_filter', lang)}：{filter_desc}")
        filter_label.setObjectName("ReportMuted")
        title_box.addWidget(filter_label)
        h.addLayout(title_box)
        h.addStretch()
        h.addWidget(QLabel(ui_text("report_status", lang).format(
            songs=f"{report['song_count']:,}",
            tokens=f"{report['token_count']:,}",
        )))
        self.report_body.addWidget(header)

        grid = QGridLayout()
        grid.setSpacing(12)
        cards = [
            (ui_text("report_song_count", lang), f"{report['song_count']:,}", ""),
            (ui_text("report_line_count", lang), f"{report['utterance_count']:,}", ""),
            (ui_text("report_token_count", lang), f"{report['token_count']:,}", ui_text("report_content_tokens", lang)),
            (ui_text("report_type_count", lang), f"{report['type_count']:,}", ui_text("report_unique_lemma", lang)),
        ]
        for i, (label, value, note) in enumerate(cards):
            grid.addWidget(self._make_report_card(label, value, note), 0, i)
        self.report_body.addLayout(grid)

        sttr = report.get("sttr")
        chunk = report.get("sttr_chunk")
        sttr_text = f"{sttr:.4f}" if sttr is not None else "—"
        sttr_note = f"{ui_text('report_sttr_note', lang)} / {chunk} tokens" if chunk else ui_text("report_sttr_note", lang)
        diversity = QGridLayout()
        diversity.setSpacing(12)
        diversity_cards = [
            ("TTR", f"{report['ttr']:.4f}", "Types / Tokens"),
            ("STTR", sttr_text, sttr_note),
            (ui_text("report_hapax", lang), f"{report['hapax_ratio'] * 100:.1f}%",
             ui_text("report_hapax_note", lang).format(count=f"{report['hapax_count']:,}")),
            (ui_text("report_avg_song", lang), f"{report['avg_types_per_song']:.0f}", "Types / Song"),
            (ui_text("report_avg_line", lang), f"{report['avg_tokens_per_line']:.1f}", "Tokens / Line"),
        ]
        for i, (label, value, note) in enumerate(diversity_cards):
            diversity.addWidget(self._make_report_card(label, value, note), 0, i)
        div_panel = self._make_report_section(ui_text("report_diversity", lang))
        div_panel.layout().addLayout(diversity)
        self.report_body.addWidget(div_panel)

        pos_rows = report.get("pos_dist", [])
        pos_panel = self._make_report_section(ui_text("report_pos", lang))
        pos_table = self._make_report_table(ui_text("report_table_pos", lang) + [""], len(pos_rows))
        pos_table.horizontalHeader().setSectionResizeMode(0, QHeaderView.ResizeMode.ResizeToContents)
        pos_table.horizontalHeader().setSectionResizeMode(1, QHeaderView.ResizeMode.ResizeToContents)
        pos_table.horizontalHeader().setSectionResizeMode(5, QHeaderView.ResizeMode.Stretch)
        max_pct = pos_rows[0]["token_pct"] if pos_rows else 1
        for r, row in enumerate(pos_rows):
            values = [
                row["pos"],
                _POS_JA.get(row["pos"], row["pos"]),
                f"{row['token_count']:,}",
                f"{row['token_pct']:.1f}%",
                f"{row['type_count']:,}",
            ]
            for c, value in enumerate(values):
                item = QTableWidgetItem(value)
                if c >= 2:
                    item.setTextAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
                pos_table.setItem(r, c, item)
            pos_table.setCellWidget(r, 5, self._make_report_bar(row["token_pct"], max_pct))
        pos_panel.layout().addWidget(pos_table)
        self.report_body.addWidget(pos_panel)

        coverage = report.get("coverage", [])
        top_words = report.get("top_words", [])

        cov_panel = self._make_report_section(ui_text("report_coverage", lang))
        cov_grid = QGridLayout()
        cov_grid.setSpacing(12)
        for i, cov in enumerate(coverage):
            card = self._make_report_card(f"Top {cov['n']:,}", f"{cov['pct']:.1f}%", "")
            card.layout().addWidget(self._make_report_bar(cov["pct"], 100))
            cov_grid.addWidget(card, 0, i)
        cov_panel.layout().addLayout(cov_grid)
        self.report_body.addWidget(cov_panel)

        top_panel = self._make_report_section(ui_text("report_top_words", lang))
        top_table = self._make_report_table(ui_text("report_table_top", lang) + [""], len(top_words))
        top_table.horizontalHeader().setSectionResizeMode(0, QHeaderView.ResizeMode.ResizeToContents)
        top_table.horizontalHeader().setSectionResizeMode(1, QHeaderView.ResizeMode.Stretch)
        top_table.horizontalHeader().setSectionResizeMode(3, QHeaderView.ResizeMode.Stretch)
        max_freq = top_words[0][1] if top_words else 1
        for r, (lemma, freq) in enumerate(top_words):
            rank = QTableWidgetItem(str(r + 1))
            rank.setTextAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
            top_table.setItem(r, 0, rank)
            top_table.setItem(r, 1, QTableWidgetItem(str(lemma)))
            freq_item = QTableWidgetItem(f"{freq:,}")
            freq_item.setTextAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
            top_table.setItem(r, 2, freq_item)
            top_table.setCellWidget(r, 3, self._make_report_bar(freq, max_freq))
        top_panel.layout().addWidget(top_table)
        self.report_body.addWidget(top_panel)
        self.report_body.addStretch()
        fade_in_widget(self.report_content, 120)

    # ------------------------------------------------------------------ Settings
    def _apply_saved_settings(self):
        self.ctx_len_spin.setValue(self._cfg.get("ctx_len", 40))
        self.chk_cross_line.setChecked(self._cfg.get("cross_line", False))
        self.pos_combo.setCurrentIndex(self._cfg.get("pos_idx", 0))
        fam, fsz = self._cfg["font_family"], self._cfg["font_size"]
        self.input.setFont(QFont(fam, fsz + 1))
        for i, w in enumerate(self._cfg.get("search_col_widths", [])):
            self.table.setColumnWidth(i + 1, w)

    def _save_col_widths(self):
        self._cfg["search_col_widths"] = [
            self.table.columnWidth(i) for i in range(1, 6)
        ]

    def _sync_speed_buttons(self):
        rate = normalize_playback_rate(getattr(self, "_playback_rate", self._cfg.get("playback_rate", 1.0)))
        if hasattr(self, "speed_control"):
            self.speed_control.blockSignals(True)
            self.speed_control.setRate(rate, emit=False)
            self.speed_control.blockSignals(False)

    def _refresh_playback_rate_from_settings(self):
        cfg = load_settings()
        self._playback_rate = normalize_playback_rate(cfg.get("playback_rate", self._playback_rate))
        self._player.setPlaybackRate(self._playback_rate)
        self._sync_speed_buttons()

    def _set_playback_rate(self, rate: float):
        self._playback_rate = normalize_playback_rate(rate)
        self._player.setPlaybackRate(self._playback_rate)
        self._cfg["playback_rate"] = self._playback_rate
        save_settings(self._cfg)
        self._sync_speed_buttons()
        self.statusBar().showMessage(f"{ui_text('playback_speed', self.lang)}: {playback_rate_label(self._playback_rate)}")

    def _toggle_filter_panel(self):
        visible = not self.filter_panel.isVisible()
        self.filter_panel.setVisible(visible)
        if visible:
            fade_in_widget(self.filter_panel, 120)
        self.btn_filter_toggle.setText(
            ui_text("filter_open", self.lang) if visible
            else ui_text("filter_closed", self.lang)
        )

    def open_dict_manager(self):
        self._show_page("dict")

    def refresh_theme(self, theme: str):
        """Refresh theme-aware pages that keep local or manually painted colors."""
        self._preview_theme_name = "light" if theme == "light" else "dark"
        for page_name in ("search_page", "stats_page"):
            page = getattr(self, page_name, None)
            if page is not None:
                self._apply_modern_page_style(page, self._preview_theme_name)
        if hasattr(self, "report_content"):
            self._apply_report_style(self._preview_theme_name)
        for widget in list(getattr(self, "_embedded_tool_widgets", {}).values()):
            refresh = getattr(widget, "refresh_theme", None)
            if callable(refresh):
                refresh(self._preview_theme_name)

    def _preview_theme(self, theme: str):
        app = QApplication.instance()
        if app is not None:
            apply_theme(app, theme)

    def _cancel_theme_preview(self, dlg: FontSettingsDialog):
        theme = getattr(dlg, "_original_theme", self._cfg.get("theme", "dark"))
        app = QApplication.instance()
        if app is not None:
            apply_theme(app, theme)
        self._preview_theme_name = None
        self._on_embedded_tool_closed("settings")

    def _apply_font_settings_dialog(self, dlg: FontSettingsDialog):
        family, size, wc_path, theme, yomitan_path, ui_language, keyword_color, custom_font_paths = dlg.result_font()
        old_language = self._cfg.get("ui_language", "zh")
        self._cfg["font_family"]     = family
        self._cfg["font_size"]       = size
        self._cfg["wc_font_path"]    = wc_path
        self._cfg["theme"]           = theme
        self._cfg["yomitan_zh_dict"] = yomitan_path
        self._cfg["ui_language"]     = ui_language
        self._cfg["keyword_color"]   = keyword_color
        self._cfg["custom_font_paths"] = custom_font_paths
        save_settings(self._cfg)
        from PyQt6.QtWidgets import QApplication
        app = QApplication.instance()
        load_custom_fonts(custom_font_paths)
        app.setFont(QFont(family, size))
        apply_theme(app, theme)
        self._preview_theme_name = None
        if ui_language != old_language:
            self._rebuild_ui_after_settings("settings")
        else:
            self.input.setFont(QFont(family, size + 1))
            if self._results:
                self._on_results(self._results)
            if self._stats_data:
                self._on_stats_results(self._stats_data)
            self._on_embedded_tool_closed("settings")

    def open_font_settings(self):
        self._show_page("settings")

    def open_anki_export(self):
        self._show_page("anki")

    def _generate_report(self):
        import subprocess, pathlib as _pl
        script = _pl.Path(__file__).parent / "generate_report.py"
        out    = _pl.Path(__file__).parent / "output" / "corpus_report.html"
        import sys as _sys
        try:
            result = subprocess.run(
                [_sys.executable, str(script)],
                capture_output=True, text=True, encoding="utf-8"
            )
            if result.returncode == 0:
                import os
                os.startfile(str(out))
            else:
                from PyQt6.QtWidgets import QMessageBox
                QMessageBox.critical(self, "エラー", result.stderr or "生成失败")
        except Exception as e:
            from PyQt6.QtWidgets import QMessageBox
            QMessageBox.critical(self, "エラー", str(e))

    def open_song_manager(self):
        self._show_page("songs")
        self._load_artists()

    def closeEvent(self, event):
        self._cfg["window_width"]  = self.width()
        self._cfg["window_height"] = self.height()
        self._cfg["ctx_len"]       = self.ctx_len_spin.value()
        self._cfg["cross_line"]    = self.chk_cross_line.isChecked()
        self._cfg["pos_idx"]       = self.pos_combo.currentIndex()
        save_settings(self._cfg)
        super().closeEvent(event)

    # ------------------------------------------------------------------ アーティスト一覧
    def _load_artists(self):
        try:
            conn = sqlite3.connect(str(DB_PATH))
            rows = conn.execute("SELECT artist FROM songs").fetchall()
            conn.close()
        except sqlite3.OperationalError:
            rows = []
            self.statusBar().showMessage(
                "未找到语料库数据。请先导入音频/LRC 并构建 corpus.db。"
            )
        counts: dict[str, int] = {}
        for (artist,) in rows:
            for a in artist.split("/"):
                a = a.strip()
                if a:
                    counts[a] = counts.get(a, 0) + 1
        labeled = [
            (f"{name} ({cnt}首)", name)
            for name, cnt in sorted(counts.items(), key=lambda x: (-x[1], x[0]))
        ]
        self.artist_widget.setItemsLabeled(labeled)
        self.stats_artist_widget.setItemsLabeled(labeled)
        self.report_artist_widget.setItemsLabeled(labeled)

    # ------------------------------------------------------------------ 検索
    @staticmethod
    def _parse_keywords(raw: str) -> list[str]:
        """'(A|B|C)' または 'A|B|C' を分割してリストを返す。"""
        s = raw.strip()
        if s.startswith("(") and s.endswith(")"):
            s = s[1:-1]
        return [k.strip() for k in s.split("|") if k.strip()]

    def do_search(self):
        raw = self.input.text().strip()
        if not raw:
            return
        keywords = self._parse_keywords(raw)
        mode          = "surface" if self.rb_surface.isChecked() else "lemma"
        pos_filter    = self.pos_combo.currentData()
        artist_filter = self.artist_widget.selectedValues()
        cross_line    = self.chk_cross_line.isChecked()
        jp_only       = self.chk_jp_only.isChecked()
        dedup         = self.chk_dedup.isChecked()
        self.btn_search.setEnabled(False)
        self.statusBar().showMessage(
            ui_text("search_loading", self.lang).format(keywords="|".join(keywords)))
        self._worker = SearchWorker(keywords, mode, pos_filter, artist_filter,
                                    cross_line, jp_only, dedup)
        self._worker.results_ready.connect(self._on_results)
        self._worker.start()

    def _on_results(self, results: list[dict]):
        self._results = results
        self.table.setRowCount(len(results))
        red       = QBrush(QColor(keyword_color_for(self._cfg)))
        fam, fsz  = self._cfg["font_family"], self._cfg["font_size"]
        bold_font = QFont(fam, fsz, QFont.Weight.Bold)
        ctx_font  = QFont(fam, fsz)

        # Bulk-query which utterance_ids have manual corrections
        utt_ids = [r.get("utterance_id") for r in results if r.get("utterance_id")]
        corrected_ids: set = set()
        if utt_ids:
            try:
                conn = sqlite3.connect(str(DB_PATH))
                ph = ",".join("?" * len(utt_ids))
                corrected_ids = {
                    r[0] for r in conn.execute(
                        f"SELECT utterance_id FROM token_corrections WHERE utterance_id IN ({ph})",
                        utt_ids).fetchall()
                }
                conn.close()
            except Exception:
                pass
        is_dark = self._cfg.get("theme", "dark") == "dark"
        pencil_brush = QBrush(QColor("#2d4a2a" if is_dark else "#d4edda"))

        for i, r in enumerate(results):
            mins = int(r["time_sec"] // 60)
            secs = r["time_sec"] % 60
            btn = QPushButton("▶")
            btn.setFlat(True)
            btn.setToolTip("播放此行")
            btn.setStyleSheet(
                "QPushButton { color: #5dade2; font-size: 13px; border: none; background: transparent; }"
                "QPushButton:hover { color: #85c1e9; }"
                "QPushButton:pressed { color: #2e86c1; }"
            )
            btn.clicked.connect(lambda checked, row=i: self.on_double_click(row, 0))
            self.table.setCellWidget(i, 0, btn)
            self.table.setItem(i, 1, QTableWidgetItem(r["artist"]))
            self.table.setItem(i, 2, QTableWidgetItem(r["title"]))
            repeat = r.get("repeat_count", 1)
            time_txt = f"{mins:02d}:{secs:05.2f}"
            if repeat > 1:
                time_txt += f"  ×{repeat}"
            time_item = QTableWidgetItem(time_txt)
            if repeat > 1:
                time_item.setToolTip(f"此行歌词在曲中共出现 {repeat} 次，显示首次出现时刻")
            is_corrected = r.get("utterance_id") in corrected_ids
            if is_corrected:
                time_item.setText(f"✏ {time_txt}")
                time_item.setToolTip("此行分词已手动校正"
                                     + (f"（×{repeat}）" if repeat > 1 else ""))
            self.table.setItem(i, 3, time_item)
            n = self.ctx_len_spin.value()
            left_disp  = r["left"][-n:]  if len(r["left"])  > n else r["left"]
            right_disp = r["right"][:n] if len(r["right"]) > n else r["right"]

            left_item = QTableWidgetItem(left_disp)
            left_item.setTextAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
            left_item.setFont(ctx_font)
            left_item.setToolTip(r["left"])
            self.table.setItem(i, 4, left_item)
            kw_item = QTableWidgetItem(r["match"])
            kw_item.setForeground(red)
            kw_item.setFont(bold_font)
            kw_item.setTextAlignment(Qt.AlignmentFlag.AlignCenter | Qt.AlignmentFlag.AlignVCenter)
            if is_corrected:
                kw_item.setBackground(pencil_brush)
                kw_item.setToolTip("此行分词已手动校正（右键可重新编辑）")
            self.table.setItem(i, 5, kw_item)
            right_item = QTableWidgetItem(right_disp)
            right_item.setFont(ctx_font)
            right_item.setToolTip(r["right"])
            self.table.setItem(i, 6, right_item)

        self.lbl_count.setText(ui_text("results_count", self.lang).format(n=len(results)))
        self.btn_search.setEnabled(True)
        self.btn_export.setEnabled(len(results) > 0)
        self.statusBar().showMessage(ui_text("search_done", self.lang).format(n=len(results)))

    # ------------------------------------------------------------------ 词频
    def load_stats(self):
        pos_filter    = self.stats_pos_combo.currentData()
        artist_filter = self.stats_artist_widget.selectedValues()
        jp_only       = self.chk_stats_jp_only.isChecked()
        self.btn_load_stats.setEnabled(False)
        self.statusBar().showMessage(ui_text("stats_loading", self.lang))
        self._stats_worker = StatsWorker(pos_filter, artist_filter, jp_only)
        self._stats_worker.results_ready.connect(self._on_stats_results)
        self._stats_worker.start()

    def _on_stats_results(self, rows):
        self.stats_table.setRowCount(len(rows))
        tc   = _THEME_COLORS[self._cfg.get("theme", "dark")]
        blue = QBrush(QColor(tc["lemma"]))
        center = Qt.AlignmentFlag.AlignCenter | Qt.AlignmentFlag.AlignVCenter

        for i, (lemma, pos, freq, song_count, surfaces) in enumerate(rows):
            lemma_item = QTableWidgetItem(lemma)
            lemma_item.setForeground(blue)
            self.stats_table.setItem(i, 0, lemma_item)
            self.stats_table.setItem(i, 1, QTableWidgetItem(pos))

            jlpt_it = QTableWidgetItem("…")
            jlpt_it.setTextAlignment(center)
            self.stats_table.setItem(i, 2, jlpt_it)

            freq_item = QTableWidgetItem(str(freq))
            freq_item.setTextAlignment(center)
            self.stats_table.setItem(i, 3, freq_item)

            song_item = QTableWidgetItem(str(song_count))
            song_item.setTextAlignment(center)
            self.stats_table.setItem(i, 4, song_item)

            surface_list = (surfaces or "").split(",")[:5]
            self.stats_table.setItem(i, 5, QTableWidgetItem("、".join(surface_list)))

        self._stats_data = rows
        self.lbl_stats_count.setText(ui_text("stats_count", self.lang).format(n=len(rows)))
        self.btn_load_stats.setEnabled(True)
        self.btn_wordcloud.setEnabled(bool(rows))

        # Build O(1) row-index for JLPT updates
        self._stats_row_index = {r[0]: i for i, r in enumerate(rows)}

        # Stop any previous worker
        if hasattr(self, '_stats_jlpt_worker') and self._stats_jlpt_worker \
                and self._stats_jlpt_worker.isRunning():
            self._stats_jlpt_worker.requestInterruption()
            self._stats_jlpt_worker.wait(300)
        self._stats_jlpt_map = {}
        lemmas = [r[0] for r in rows]

        # Pre-load cache synchronously so already-known levels show instantly
        try:
            _ensure_jlpt_cache_table()
            conn = sqlite3.connect(str(DB_PATH))
            ph = ",".join("?" * len(lemmas))
            raw_cached = dict(conn.execute(
                f"SELECT lemma, level FROM jlpt_cache WHERE lemma IN ({ph})", lemmas
            ).fetchall())
            conn.close()
        except Exception:
            raw_cached = {}

        center = Qt.AlignmentFlag.AlignCenter | Qt.AlignmentFlag.AlignVCenter
        for lm, raw_lvl in raw_cached.items():
            lvl = raw_lvl.replace("JLPT-", "") if raw_lvl.startswith("JLPT-") else raw_lvl
            self._stats_jlpt_map[lm] = lvl
            i = self._stats_row_index.get(lm, -1)
            if i >= 0:
                it = QTableWidgetItem(lvl if lvl else "—")
                it.setTextAlignment(center)
                self.stats_table.setItem(i, 2, it)

        uncached = [lm for lm in lemmas if lm not in raw_cached]
        n_cached = len(raw_cached)
        self.statusBar().showMessage(
            ui_text("stats_done", self.lang).format(n=len(rows)) + "  |  "
            + (ui_text("stats_jlpt_cached", self.lang).format(
                cached=n_cached, uncached=len(uncached))
               if uncached else ui_text("stats_jlpt_all_cached", self.lang).format(cached=n_cached))
        )
        if not uncached:
            return

        self._stats_jlpt_worker = JlptFetchWorker(uncached)
        self._stats_jlpt_worker.result.connect(self._on_stats_jlpt_result)
        self._stats_jlpt_worker.done.connect(self._on_stats_jlpt_done)
        self._stats_jlpt_worker.start()

    def _on_stats_jlpt_result(self, lemma: str, level: str):
        self._stats_jlpt_map[lemma] = level
        i = self._stats_row_index.get(lemma, -1)
        if i < 0:
            return
        lvl_it = QTableWidgetItem(level if level else "—")
        lvl_it.setTextAlignment(
            Qt.AlignmentFlag.AlignCenter | Qt.AlignmentFlag.AlignVCenter)
        self.stats_table.setItem(i, 2, lvl_it)

    def _on_stats_jlpt_done(self):
        self.statusBar().showMessage(
            ui_text("stats_jlpt_done", self.lang).format(n=self.stats_table.rowCount()))

    def _on_stats_double_click(self, row: int, _col: int):
        lemma_item = self.stats_table.item(row, 0)
        if not lemma_item:
            return
        self.input.setText(lemma_item.text())
        self.rb_lemma.setChecked(True)
        self._show_page("search")
        self.do_search()

    def _show_wordcloud(self):
        if not self._stats_data:
            return
        word_freq = {row[0]: row[2] for row in self._stats_data if row[2] > 0}
        wc_font = self._cfg.get("wc_font_path", _WC_FONT)
        theme   = self._cfg.get("theme", "dark")
        dlg = WordCloudDialog(word_freq, wc_font, theme, self)
        dlg.exec()

    # ------------------------------------------------------------------ 音声整理
    def reorganize_audio(self):
        ans = QMessageBox.question(
            self, "音声ファイルを整理",
            "全曲の音声ファイルを\nraw/audio/{歌手}/{id}.flac\nに整理します。\n\n"
            "・すでに AUDIO フォルダ内のファイルは移動\n"
            "・外部のファイルはコピー（元ファイルは残ります）\n\n続けますか？"
        )
        if ans != QMessageBox.StandardButton.Yes:
            return
        self.btn_reorg.setEnabled(False)
        self.statusBar().showMessage("音声ファイルを整理中…")
        self._reorg_worker = ReorganizeWorker()
        self._reorg_worker.progress.connect(self.statusBar().showMessage)
        self._reorg_worker.finished.connect(self._on_reorg_finished)
        self._reorg_worker.start()

    def _on_reorg_finished(self, ok: bool, msg: str):
        self.btn_reorg.setEnabled(True)
        self.statusBar().showMessage(msg)
        if not ok:
            QMessageBox.critical(self, "エラー", msg)
        else:
            self._check_missing_audio()

    # ------------------------------------------------------------------ 曲追加
    def add_song(self):
        self._show_page("import")

    def _start_add_from_dialog(self, dlg: AddSongDialog):
        songs = dlg.get_data()
        if not songs:
            return
        if hasattr(self, "btn_add_song"):
            self.btn_add_song.setEnabled(False)
        self.statusBar().showMessage("処理中…")
        self._add_worker = AddSongWorker(songs)
        self._add_worker.progress.connect(self.statusBar().showMessage)
        self._add_worker.finished.connect(self._on_add_finished)
        self._add_worker.start()

    def _on_add_finished(self, ok: bool, msg: str):
        if hasattr(self, "btn_add_song"):
            self.btn_add_song.setEnabled(True)
        self.statusBar().showMessage(msg.splitlines()[0])
        if ok:
            self._load_artists()
            if "スキップ" in msg:
                QMessageBox.warning(self, "重複スキップ", msg)
        else:
            QMessageBox.critical(self, "エラー", msg)

    # ------------------------------------------------------------------ 音频
    def _check_missing_audio(self):
        try:
            conn = sqlite3.connect(str(DB_PATH))
            rows = conn.execute("SELECT id, artist, title, audio_path FROM songs").fetchall()
            conn.close()
        except sqlite3.OperationalError:
            self._missing_audio = []
            self.btn_repair.hide()
            return
        self._missing_audio = [
            (sid, a, t, p) for sid, a, t, p in rows
            if not pathlib.Path(p).exists()
        ]
        if self._missing_audio:
            names = "、".join(f"{a}《{t}》" for _, a, t, _ in self._missing_audio[:3])
            extra = f" 他{len(self._missing_audio)-3}件" if len(self._missing_audio) > 3 else ""
            self.statusBar().showMessage(
                f"⚠ 音声ファイルが見つかりません：{names}{extra}"
            )
            self.btn_repair.show()
        else:
            self.btn_repair.hide()

    def repair_audio(self):
        if not self._missing_audio:
            self._check_missing_audio()
        if not self._missing_audio:
            self.statusBar().showMessage("音声ファイルはすべて正常です")
            return
        dlg = RepairAudioDialog(self._missing_audio, self)
        if dlg.exec() == QDialog.DialogCode.Accepted and dlg.resolved_count() > 0:
            self.statusBar().showMessage(f"修復完了：{dlg.resolved_count()} 曲")
            self._check_missing_audio()

    def _relink_audio(self, r: dict):
        ans = QMessageBox.question(
            self, "音声ファイルが見つかりません",
            f"{r['artist']}《{r['title']}》\n\n{r['audio_path']}\n\nファイルを移動または削除しましたか？\n新しいファイルを指定しますか？"
        )
        if ans != QMessageBox.StandardButton.Yes:
            return
        new_path, _ = QFileDialog.getOpenFileName(
            self, "音声ファイルを選択", "",
            "音声ファイル (*.flac *.mp3 *.wav *.m4a);;すべてのファイル (*)"
        )
        if not new_path:
            return
        # DB 更新
        conn = sqlite3.connect(str(DB_PATH))
        conn.execute(
            "UPDATE songs SET audio_path=? WHERE artist=? AND title=?",
            (new_path, r["artist"], r["title"])
        )
        conn.commit()
        conn.close()
        # CSV 更新
        with open(CSV_PATH, encoding="utf-8-sig", newline="") as f:
            all_rows = list(csv.DictReader(f))
        fieldnames = ["id", "title", "artist", "year", "album", "genre", "audio_path"]
        for row in all_rows:
            if row["artist"] == r["artist"] and row["title"] == r["title"]:
                row["audio_path"] = new_path
        with open(CSV_PATH, "w", encoding="utf-8-sig", newline="") as f:
            writer = csv.DictWriter(f, fieldnames=fieldnames)
            writer.writeheader()
            writer.writerows(all_rows)
        # メモリ内の結果も更新
        r["audio_path"] = new_path
        self.statusBar().showMessage(f"再リンク完了：{pathlib.Path(new_path).name}")

    def _on_kwic_context_menu(self, pos):
        from PyQt6.QtWidgets import QMenu
        row = self.table.rowAt(pos.y())
        if row < 0 or row >= len(self._results):
            return
        r = self._results[row]
        uid = r.get("utterance_id")
        if not uid:
            return
        menu = QMenu(self)
        act = menu.addAction("分词校正…")
        if menu.exec(self.table.viewport().mapToGlobal(pos)) == act:
            from dialogs.token_correction import TokenCorrectionDialog
            dlg = TokenCorrectionDialog(uid, r["text"],
                                        artist=r.get("artist", ""),
                                        title=r.get("title", ""), parent=self)
            dlg.exec()

    def on_double_click(self, row: int, _col: int):
        if row >= len(self._results):
            return
        r = self._results[row]
        if not pathlib.Path(r["audio_path"]).exists():
            self._relink_audio(r)
            return
        self._loop_start_ms = int(r["time_sec"] * 1000)
        self._loop_end_ms   = self._get_end_ms(r)
        self._seek_ms = self._loop_start_ms
        self._player.setSource(QUrl.fromLocalFile(r["audio_path"]))
        self._player.setPlaybackRate(self._playback_rate)
        self._player.play()
        QTimer.singleShot(250, self._seek)
        self.lbl_now.setText(f"▶ {r['artist']}《{r['title']}》{r['time_sec']:.1f}s — {r['text']}")
        self.btn_play.setText("⏸")
        if self.chk_loop.isChecked():
            self._loop_timer.start()

    def _seek(self):
        self._player.setPosition(self._seek_ms)

    def _is_playing(self) -> bool:
        return self._player.playbackState() == QMediaPlayer.PlaybackState.PlayingState

    def _on_player_error(self, error, error_string: str):
        if error == QMediaPlayer.Error.NoError:
            return
        self.btn_play.setText("▶")
        self.statusBar().showMessage(f"音频播放失败：{error_string or error.name}")

    def _get_end_ms(self, r: dict) -> int:
        conn = sqlite3.connect(str(DB_PATH))
        row = conn.execute("""
            SELECT u.time_sec FROM utterances u
            JOIN songs s ON s.id = u.song_id
            WHERE s.artist = ? AND s.title = ? AND u.time_sec > ?
            ORDER BY u.time_sec LIMIT 1
        """, (r["artist"], r["title"], r["time_sec"])).fetchone()
        conn.close()
        if row:
            return int(row[0] * 1000) - 80
        return self._loop_start_ms + 8000

    def _check_loop(self):
        if self._player.position() >= self._loop_end_ms:
            self._player.setPosition(self._loop_start_ms)

    def _on_loop_toggled(self, checked: bool):
        if checked and self._is_playing():
            self._loop_timer.start()
        else:
            self._loop_timer.stop()

    def toggle_play_pause(self):
        if self._is_playing():
            self._player.pause()
            self.btn_play.setText("▶")
        else:
            self._player.play()
            self.btn_play.setText("⏸")

    # ------------------------------------------------------------------ 語料統計報告
    def generate_report(self):
        artist_filter = self.report_artist_widget.selectedValues()
        jp_only       = self.chk_report_jp_only.isChecked()
        self.btn_gen_report.setEnabled(False)
        self.btn_export_report.setEnabled(False)
        self.lbl_report_status.setText(ui_text("report_loading", self.lang))
        self._report_worker = CorpusReportWorker(artist_filter, jp_only)
        self._report_worker.finished.connect(self._on_report_done)
        self._report_worker.start()

    def _on_report_done(self, report: dict):
        self._last_report = report
        self.btn_gen_report.setEnabled(True)
        self.btn_export_report.setEnabled(True)
        self.lbl_report_status.setText(
            ui_text("report_status", self.lang).format(
                songs=f"{report['song_count']:,}",
                tokens=f"{report['token_count']:,}",
            )
        )
        self._render_report_dashboard(report)

    def _render_report_html(self, report: dict) -> str:
        theme = self._cfg.get("theme", "dark")
        fam   = self._cfg.get("font_family", "Yu Gothic UI")
        fsz   = self._cfg.get("font_size", 11)
        if theme == "dark":
            bg = "#2b2b2b"; fg = "#e8e8e8"; alt = "#3a3a3a"
            brd = "#555555"; hdr_bg = "#252525"; num_c = "#7ec8e3"
            bar_bg = "#444444"; accent = "#5dade2"
        else:
            bg = "#f0f0f0"; fg = "#1a1a1a"; alt = "#e8e8e8"
            brd = "#c8c8c8"; hdr_bg = "#d8d8d8"; num_c = "#1a6fa8"
            bar_bg = "#d0d0d0"; accent = "#1a6fa8"

        def card(label, value, note=""):
            note_html = f"<div style='font-size:10px;color:#888;margin-top:2px'>{note}</div>" if note else ""
            return (
                f"<td style='padding:10px 14px;text-align:center;"
                f"background:{alt};border:1px solid {brd};border-radius:6px'>"
                f"<div style='font-size:22px;font-weight:bold;color:{num_c}'>{value}</div>"
                f"<div style='font-size:11px;margin-top:3px'>{label}</div>{note_html}</td>"
                f"<td style='width:8px'></td>"
            )

        def section(title):
            return (
                f"<h3 style='color:{accent};border-bottom:1px solid {brd};"
                f"padding-bottom:5px;margin-top:18px;margin-bottom:10px'>{title}</h3>"
            )

        def th(text, align="left"):
            return (
                f"<th style='background:{hdr_bg};padding:6px 10px;"
                f"border:1px solid {brd};text-align:{align}'>{text}</th>"
            )

        def td(text, align="left", extra=""):
            return (
                f"<td style='padding:5px 10px;border:1px solid {brd};"
                f"text-align:{align}{extra}'>{text}</td>"
            )

        def bar(pct, max_pct=100):
            # QTextBrowser 不支持嵌套 div 的 width:%，用双格 table 模拟进度条
            w = max(0, min(round(pct / max_pct * 100) if max_pct else 0, 100))
            e = 100 - w
            filled = f"<td width='{w}%' style='background:{num_c};height:12px'></td>" if w else ""
            empty  = f"<td width='{e}%' style='background:{bar_bg};height:12px'></td>" if e else ""
            return (
                f"<td style='padding:5px 10px;border:1px solid {brd};width:130px'>"
                f"<table width='100%' cellspacing='0' cellpadding='0'>"
                f"<tr>{filled}{empty}</tr></table></td>"
            )

        # 生成图表（matplotlib）
        charts = self._make_charts(report, theme)

        def img(key):
            """居中嵌入图表；按 logical px 宽度设定尺寸保证 HiDPI 清晰。"""
            data = charts.get(key)
            if not data:
                return ""
            uri, w = data
            return f"<div align='center' style='margin:10px 0'><img src='{uri}' width='{w}'/></div>"

        # Filter description
        artists = report.get("artist_filter") or []
        filter_desc = "、".join(artists) if artists else "全歌手"
        if report.get("jp_only"):
            filter_desc += "（仅日文）"

        # TTR & STTR
        ttr_str  = f"{report['ttr']:.4f}"
        sttr_val = report.get("sttr")
        chunk    = report.get("sttr_chunk")
        sttr_str = f"{sttr_val:.4f}" if sttr_val is not None else "—"
        sttr_note = f"片段={chunk}词" if chunk else "tokens不足"
        hapax_str = f"{report['hapax_ratio']*100:.1f}%"
        avg_song  = f"{report['avg_types_per_song']:.0f}"
        avg_line  = f"{report['avg_tokens_per_line']:.1f}"

        # ---- Section 1: 基本統計 ----
        s1 = section("基本統計")
        s1 += (
            f"<table cellspacing='0' cellpadding='0' style='border-collapse:separate;"
            f"border-spacing:8px;width:100%'><tr>"
            + card("曲数", f"{report['song_count']:,}")
            + card("歌詞行数", f"{report['utterance_count']:,}")
            + card("トークン数", f"{report['token_count']:,}", "実質語（PUNCT除く）")
            + card("タイプ数", f"{report['type_count']:,}", "ユニーク lemma 数")
            + "</tr></table>"
        )

        # ---- Section 2: 词汇多样性 ----
        s2 = section("語彙の多様性指標")
        s2 += (
            f"<table cellspacing='0' cellpadding='0' style='border-collapse:separate;"
            f"border-spacing:8px;width:100%'><tr>"
            + card("TTR", ttr_str, "Types/Tokens")
            + card(f"STTR ({sttr_note})", sttr_str, "標準化 TTR")
            + card("Hapax 比率", hapax_str,
                   f"{report['hapax_count']:,} 語が1回のみ出現")
            + card("平均曲語彙量", avg_song, "Types/曲")
            + card("平均行語数", avg_line, "Tokens/行")
            + "</tr></table>"
        )

        # ---- Section 3: 品词分布（表格 + 扇形图）----
        s3_title = section("品詞分布 (POS Distribution)")
        pos_rows = report["pos_dist"]
        max_pct  = pos_rows[0]["token_pct"] if pos_rows else 1
        pos_table = (
            f"<table cellspacing='0' cellpadding='0' style='border-collapse:collapse'>"
            f"<thead><tr>"
            + th("品詞") + th("和名") + th("トークン数", "right")
            + th("比率", "right") + th("比率バー") + th("タイプ数", "right")
            + "</tr></thead><tbody>"
        )
        for i, row in enumerate(pos_rows):
            row_bg = bg if i % 2 == 0 else alt
            pos_ja = _POS_JA.get(row["pos"], row["pos"])
            pos_table += (
                f"<tr style='background:{row_bg}'>"
                + td(f"<b>{row['pos']}</b>")
                + td(pos_ja)
                + td(f"{row['token_count']:,}", "right")
                + td(f"{row['token_pct']:.1f}%", "right")
                + bar(row["token_pct"], max_pct)
                + td(f"{row['type_count']:,}", "right")
                + "</tr>"
            )
        pos_table += "</tbody></table>"
        s3 = s3_title + pos_table + img("pos_pie")

        # ---- Section 4: 覆盖率（表格 + 柱状图）----
        s4_title = section("高頻語 Top-N 覆蓋率")
        s4_note  = (
            "<p style='font-size:11px;color:#888;margin-top:0'>上位 N 語の lemma が"
            "全トークンの何 % を占めるか。語彙学習の優先度設計に活用できます。</p>"
        )
        cov_table = (
            f"<table cellspacing='0' style='border-collapse:collapse;width:260px'>"
            f"<thead><tr>{th('Top N')}{th('覆蓋率', 'right')}{th('覆蓋バー')}"
            f"</tr></thead><tbody>"
        )
        for i, cov in enumerate(report["coverage"]):
            row_bg = bg if i % 2 == 0 else alt
            cov_table += (
                f"<tr style='background:{row_bg}'>"
                + td(f"Top {cov['n']:,}")
                + td(f"{cov['pct']:.1f}%", "right")
                + bar(cov["pct"], 100)
                + "</tr>"
            )
        cov_table += "</tbody></table>"
        s4 = s4_title + s4_note + cov_table + img("coverage_bar")

        # ---- Section 5: Top 20 词元（图表为主）----
        s5 = section("高頻語元 Top 20")
        top_chart = img("top_words_bar")
        if top_chart:
            s5 += top_chart
        else:
            top = report.get("top_words", [])
            if top:
                max_freq = top[0][1]
                s5 += (
                    f"<table cellspacing='0' style='border-collapse:collapse;width:420px'>"
                    f"<thead><tr>{th('#')}{th('lemma')}{th('出現数', 'right')}"
                    f"{th('出現バー')}</tr></thead><tbody>"
                )
                for i, (lemma, freq) in enumerate(top[:20]):
                    row_bg = bg if i % 2 == 0 else alt
                    s5 += (
                        f"<tr style='background:{row_bg}'>"
                        + td(str(i + 1), "right")
                        + f"<td style='padding:5px 10px;border:1px solid {brd};"
                        f"color:{num_c};font-weight:bold'>{lemma}</td>"
                        + td(f"{freq:,}", "right")
                        + bar(freq, max_freq)
                        + "</tr>"
                    )
                s5 += "</tbody></table>"
            else:
                s5 += "<p>データなし</p>"

        html = (
            f"<html><body style='background:{bg};color:{fg};"
            f"font-family:\"{fam}\";font-size:{fsz}px;margin:0;padding:16px'>"
            f"<div style='color:#888;font-size:11px;margin-bottom:8px'>"
            f"フィルター：{filter_desc}</div>"
            + s1 + s2 + s3 + s4 + s5
            + "</body></html>"
        )
        return html

    def _make_charts(self, report: dict, theme: str) -> dict:
        """Generate matplotlib charts → (base64 uri, logical_px_width) tuples.
        Uses screen DPR for physical resolution so images are sharp on HiDPI displays."""
        try:
            import matplotlib
            try:
                matplotlib.use("Agg")
            except Exception:
                pass  # backend already initialised; proceed and hope it's compatible
            import matplotlib.pyplot as plt
            import matplotlib.font_manager as fm
            from io import BytesIO
            import base64
            from PyQt6.QtWidgets import QApplication
        except Exception:
            return {}

        dpr = QApplication.instance().devicePixelRatio()
        # base_dpi=100 → logical px = figsize_inches × 100;  physical px = figsize × 100 × dpr
        render_dpi = int(100 * dpr)

        wc_font = self._cfg.get("wc_font_path", "")
        if pathlib.Path(wc_font).exists():
            try:
                fm.fontManager.addfont(wc_font)
                prop = fm.FontProperties(fname=wc_font)
                matplotlib.rcParams["font.family"] = prop.get_name()
            except Exception:
                pass

        if theme == "dark":
            bg, fg, grid_c = "#2b2b2b", "#e8e8e8", "#444444"
            palette = ["#5dade2","#ff6b6b","#58d68d","#f39c12","#a569bd",
                       "#1abc9c","#e67e22","#3498db","#2ecc71","#f1c40f",
                       "#9b59b6","#16a085","#d35400","#cb4335"]
        else:
            bg, fg, grid_c = "#f0f0f0", "#1a1a1a", "#cccccc"
            palette = ["#2980b9","#c0392b","#27ae60","#d68910","#8e44ad",
                       "#148f77","#e67e22","#2471a3","#1e8449","#b7950b",
                       "#7d3c98","#0e6655","#a04000","#922b21"]

        plt.rcParams.update({
            "figure.facecolor": bg, "axes.facecolor": bg,
            "text.color": fg, "axes.labelcolor": fg,
            "xtick.color": fg, "ytick.color": fg,
            "axes.edgecolor": grid_c, "grid.color": grid_c,
        })

        def to_uri(fig, w_inches: float) -> tuple:
            buf = BytesIO()
            fig.savefig(buf, format="png", bbox_inches="tight",
                        dpi=render_dpi, facecolor=bg)
            plt.close(fig)
            buf.seek(0)
            uri = "data:image/png;base64," + base64.b64encode(buf.read()).decode()
            return uri, int(w_inches * 100)   # (uri, logical_width_px)

        charts = {}

        # 1. 品词扇形图  figsize=(6.5, 5) → 650 logical px wide
        pos_all = [(r["pos"], r["token_count"]) for r in report["pos_dist"] if r["token_count"] > 0]
        if pos_all:
            if len(pos_all) > 9:
                top9  = pos_all[:9]
                other = sum(c for _, c in pos_all[9:])
                if other:
                    top9.append(("その他", other))
                pos_all = top9
            labels = [_POS_JA.get(p, p) for p, _ in pos_all]
            vals   = [c for _, c in pos_all]
            fig, ax = plt.subplots(figsize=(6.5, 5.0), facecolor=bg)
            wedges, texts, autotexts = ax.pie(
                vals, labels=labels, colors=palette[:len(vals)],
                autopct=lambda p: f"{p:.1f}%" if p >= 3 else "",
                startangle=90, pctdistance=0.80,
                textprops={"color": fg, "fontsize": 10},
            )
            for at in autotexts:
                at.set(color=fg, fontsize=9)
            ax.set_title("品詞分布", color=fg, fontsize=12, pad=10)
            charts["pos_pie"] = to_uri(fig, 6.5)

        # 2. Top-N 覆盖率横向柱状图  figsize=(6, 2.8) → 600 logical px wide
        cov = report["coverage"]
        if cov:
            labels = [f"Top {c['n']:,}" for c in cov]
            vals   = [c["pct"] for c in cov]
            fig, ax = plt.subplots(figsize=(6.0, 2.8), facecolor=bg)
            bars = ax.barh(labels, vals, color=palette[0], alpha=0.85, height=0.5)
            ax.set_xlim(0, 114)
            ax.set_xlabel("覆蓋率 (%)", fontsize=10)
            ax.set_title("Top-N 覆蓋率", fontsize=12)
            ax.tick_params(labelsize=10)
            ax.grid(axis="x", linestyle="--", alpha=0.4)
            ax.spines["top"].set_visible(False)
            ax.spines["right"].set_visible(False)
            for bar, v in zip(bars, vals):
                ax.text(v + 0.6, bar.get_y() + bar.get_height() / 2,
                        f"{v:.1f}%", va="center", fontsize=10)
            charts["coverage_bar"] = to_uri(fig, 6.0)

        # 3. Top 20 词元横向柱状图  figsize=(6.5, 6.5) → 650 logical px wide
        top_words = report.get("top_words", [])[:20]
        if top_words:
            words = [w[0] for w in top_words][::-1]
            freqs = [w[1] for w in top_words][::-1]
            fig, ax = plt.subplots(figsize=(6.5, 6.5), facecolor=bg)
            bars = ax.barh(words, freqs, color=palette[2], alpha=0.85, height=0.65)
            ax.set_xlabel("出現回数", fontsize=10)
            ax.set_title("高頻語元 Top 20", fontsize=12)
            ax.tick_params(labelsize=10)
            ax.grid(axis="x", linestyle="--", alpha=0.4)
            ax.spines["top"].set_visible(False)
            ax.spines["right"].set_visible(False)
            mx = max(freqs)
            for bar, v in zip(bars, freqs):
                ax.text(v + mx * 0.01, bar.get_y() + bar.get_height() / 2,
                        str(v), va="center", fontsize=9)
            charts["top_words_bar"] = to_uri(fig, 6.5)

        return charts

    def _export_report_txt(self):
        report = getattr(self, "_last_report", None)
        if not report:
            return
        path, _ = QFileDialog.getSaveFileName(
            self, "統計レポートを保存", "corpus_report.txt", "テキストファイル (*.txt)"
        )
        if not path:
            return

        artists = report.get("artist_filter") or []
        filter_desc = "、".join(artists) if artists else "全歌手"
        sttr_val = report.get("sttr")
        sttr_str = f"{sttr_val:.4f}" if sttr_val is not None else "—"
        chunk    = report.get("sttr_chunk")

        lines = [
            "=" * 50,
            "  JPOP 語料庫 統計レポート",
            "=" * 50,
            f"フィルター：{filter_desc}",
            f"日本語のみ：{'あり' if report.get('jp_only') else 'なし'}",
            "",
            "【基本統計】",
            f"  曲数                : {report['song_count']:,}",
            f"  歌詞行数            : {report['utterance_count']:,}",
            f"  トークン数（実質語）: {report['token_count']:,}",
            f"  タイプ数（ユニーク）: {report['type_count']:,}",
            "",
            "【語彙多様性指標】",
            f"  TTR                 : {report['ttr']:.4f}  (Types/Tokens)",
            f"  STTR（{chunk}語片段）  : {sttr_str}  (標準化 TTR)",
            f"  Hapax 比率          : {report['hapax_ratio']*100:.1f}%"
            f"  ({report['hapax_count']:,} 語が1回のみ出現)",
            f"  平均曲語彙量        : {report['avg_types_per_song']:.0f} Types/曲",
            f"  平均行語数          : {report['avg_tokens_per_line']:.1f} Tokens/行",
            "",
            "【品詞分布】",
            f"  {'品詞':<10} {'和名':<10} {'トークン':>8} {'比率':>7} {'タイプ':>7}",
            "  " + "-" * 48,
        ]
        for row in report["pos_dist"]:
            pos_ja = _POS_JA.get(row["pos"], row["pos"])
            lines.append(
                f"  {row['pos']:<10} {pos_ja:<10} "
                f"{row['token_count']:>8,} {row['token_pct']:>6.1f}% "
                f"{row['type_count']:>7,}"
            )
        lines += [
            "",
            "【Top-N 覆蓋率】",
        ]
        for cov in report["coverage"]:
            lines.append(f"  Top {cov['n']:>5,} 語 : {cov['pct']:.1f}%")
        lines += [
            "",
            "【高頻語元 Top 20】",
            f"  {'順位':>4}  {'lemma':<16} {'出現数':>6}",
            "  " + "-" * 32,
        ]
        for i, (lemma, freq) in enumerate(report.get("top_words", [])[:20]):
            lines.append(f"  {i+1:>4}.  {lemma:<16} {freq:>6,}")
        lines.append("")

        with open(path, "w", encoding="utf-8") as f:
            f.write("\n".join(lines))
        self.statusBar().showMessage(f"レポート保存完了：{path}")

    def export_csv(self):
        path, _ = QFileDialog.getSaveFileName(
            self, "CSV を保存", "", "CSV ファイル (*.csv)"
        )
        if not path:
            return
        with open(path, "w", encoding="utf-8-sig", newline="") as f:
            writer = csv.writer(f)
            writer.writerow(["歌手", "曲名", "時刻(秒)", "左文脈", "キーワード", "右文脈", "全文"])
            for r in self._results:
                writer.writerow([
                    r["artist"], r["title"], r["time_sec"],
                    r["left"], r["match"], r["right"], r["text"],
                ])
        self.statusBar().showMessage(f"保存完了：{path}")


def main():
    _ensure_db_schema_on_startup()
    app = QApplication(sys.argv)
    cfg = load_settings()
    app.setStyle("Fusion")
    load_custom_fonts(cfg.get("custom_font_paths", []))
    app.setFont(QFont(cfg.get("font_family", "Microsoft YaHei UI"), cfg.get("font_size", 11)))
    apply_theme(app, cfg.get("theme", "dark"))
    win = MainWindow()
    win.show()
    sys.exit(app.exec())


def _ensure_db_schema_on_startup():
    """Lightweight startup schema check delegated to scripts/migrate_db.py."""
    try:
        candidate_dirs = [
            app_dir() / "scripts",
            pathlib.Path(__file__).resolve().parent / "scripts",
        ]
        for scripts_dir in candidate_dirs:
            if scripts_dir.exists() and str(scripts_dir) not in sys.path:
                sys.path.insert(0, str(scripts_dir))
        import migrate_db

        needs_backup = False
        needs_fts = False
        if DB_PATH.exists():
            conn = sqlite3.connect(str(DB_PATH))
            try:
                version = conn.execute("PRAGMA user_version").fetchone()[0]
                needs_fts = conn.execute(
                    "SELECT 1 FROM sqlite_master WHERE name='utterances_fts'"
                ).fetchone() is None
            finally:
                conn.close()
            needs_backup = version < migrate_db.SCHEMA_USER_VERSION
        else:
            needs_fts = True

        migrate_db.migrate_database(
            DB_PATH,
            create_backup=needs_backup,
            rebuild_fts_index=needs_fts,
        )
    except Exception as e:
        # Do not block the GUI; feature-specific code still shows errors if needed.
        print(f"[WARN] database schema check skipped: {e}", file=sys.stderr)


if __name__ == "__main__":
    main()
