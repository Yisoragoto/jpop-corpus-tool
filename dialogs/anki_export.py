"""Anki export dialog."""

from __future__ import annotations

from PyQt6.QtWidgets import QSizePolicy, QSplitter

from dialogs._bridge import install_gui_symbols

install_gui_symbols(globals())

_ANKI_UI = {
    "zh": {
        "title": "Anki 制卡导出",
        "subtitle": "筛选词汇、同步学习状态，然后发送到 Anki",
        "scope": "词汇筛选",
        "artist": "歌手",
        "all_artists": "所有歌手",
        "song": "歌曲",
        "all_songs": "所有歌曲",
        "pos": "词性",
        "noun": "名词",
        "verb": "动词",
        "adj": "形容词",
        "adv": "副词",
        "min_freq": "出现次数 >= ",
        "jp_only": "仅日文",
        "skip_kana": "排除单假名词",
        "filter": "过滤",
        "query": "查询词汇",
        "anki": "Anki 设置",
        "deck": "目标牌组",
        "card_content": "卡片内容",
        "fetch_def": "获取释义",
        "clip_audio": "音频片段",
        "examples": "例句数",
        "status": "连接状态",
        "checking": "确认中…",
        "learning": "学习状态",
        "not_synced": "未同步",
        "sync": "同步",
        "auto_uncheck": "自动取消已学",
        "advanced": "高级选项",
        "repeat": "重复与屏蔽",
        "duplicate": "重复词",
        "scope_dup": "检测范围",
        "blacklist": "屏蔽词",
        "sort": "排序",
        "primary_sort": "主排序",
        "secondary_sort": "次排序",
        "no_jlpt": "无 JLPT 时",
        "group_song": "按歌曲分组",
        "apply_sort": "应用排序",
        "word_list": "词汇列表",
        "word_hint": "查询后勾选要制卡的词",
        "lemma": "词元",
        "table_pos": "词性",
        "occurrences": "出现次数",
        "song_count": "出现曲数",
        "jpdb": "JPDB 词频",
        "anki_status": "Anki 状态",
        "selected": "{n} 个词已选",
        "select_all": "全选",
        "clear": "清空",
        "close": "关闭",
        "send": "发送到 Anki",
        "update": "更新已有卡",
        "refresh_scope": "旧牌组范围",
        "refresh_deck": "刷新旧牌组",
        "current_deck": "当前牌组",
        "main_deck": "主牌组",
        "current_only": "仅当前牌组",
        "current_children": "当前牌组+子牌组",
        "main_children": "主牌组+子牌组",
    },
    "ja": {
        "title": "Anki カード出力",
        "subtitle": "語彙を絞り込み、学習状態を同期して Anki に送信します",
        "scope": "語彙フィルター",
        "artist": "歌手",
        "all_artists": "すべての歌手",
        "song": "曲",
        "all_songs": "すべての曲",
        "pos": "品詞",
        "noun": "名詞",
        "verb": "動詞",
        "adj": "形容詞",
        "adv": "副詞",
        "min_freq": "出現回数 >= ",
        "jp_only": "日本語のみ",
        "skip_kana": "一文字かな語を除外",
        "filter": "フィルター",
        "query": "語彙を検索",
        "anki": "Anki 設定",
        "deck": "対象デッキ",
        "card_content": "カード内容",
        "fetch_def": "語義を取得",
        "clip_audio": "音声クリップ",
        "examples": "例文数",
        "status": "接続状態",
        "checking": "確認中…",
        "learning": "学習状態",
        "not_synced": "未同期",
        "sync": "同期",
        "auto_uncheck": "学習済みを自動解除",
        "advanced": "詳細オプション",
        "repeat": "重複と除外",
        "duplicate": "重複語",
        "scope_dup": "検出範囲",
        "blacklist": "除外語",
        "sort": "並び替え",
        "primary_sort": "主ソート",
        "secondary_sort": "副ソート",
        "no_jlpt": "JLPT なし",
        "group_song": "曲ごとにまとめる",
        "apply_sort": "並び替えを適用",
        "word_list": "語彙リスト",
        "word_hint": "検索後、カード化する語を選択します",
        "lemma": "語元",
        "table_pos": "品詞",
        "occurrences": "出現回数",
        "song_count": "出現曲数",
        "jpdb": "JPDB 頻度",
        "anki_status": "Anki 状態",
        "selected": "{n} 語選択中",
        "select_all": "全選択",
        "clear": "クリア",
        "close": "閉じる",
        "send": "Anki に送信",
        "update": "既存カード更新",
        "refresh_scope": "旧デッキ範囲",
        "refresh_deck": "旧デッキ更新",
        "current_deck": "現在のデッキ",
        "main_deck": "メインデッキ",
        "current_only": "現在のデッキのみ",
        "current_children": "現在+子デッキ",
        "main_children": "メイン+子デッキ",
    },
}

# ------------------------------------------------------------------ AnkiExportDialog

class AnkiExportDialog(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.lang = load_settings().get("ui_language", "zh")
        self.setWindowTitle(self._tr("title"))
        self.setMinimumWidth(1120)
        self.setMinimumHeight(700)
        self.resize(1280, 820)
        self._word_rows: list = []
        self._worker = None
        self._jlpt_map: dict = {}
        self._jpdb_freq_map: dict = {}   # lemma → int freq (batch-fetched)
        self._row_index: dict = {}       # lemma → table row index (O(1) JLPT update)
        self._jlpt_worker = None
        self._yomitan_worker = None
        self._anki_info_worker = None
        self._learning_worker = None
        self._learning_status: dict = {}
        self._learning_collection_path = ""
        self._build_ui()
        self._restore_anki_settings()
        QTimer.singleShot(60, self._fetch_anki_info)
        QTimer.singleShot(200, self._ensure_yomitan_bg)

    def _tr(self, key: str, **kwargs) -> str:
        text = _ANKI_UI.get(self.lang, _ANKI_UI["zh"]).get(
            key, _ANKI_UI["zh"].get(key, key)
        )
        return text.format(**kwargs) if kwargs else text

    def _make_chip(self, text: str, checked: bool = True) -> QPushButton:
        chip = QPushButton(text)
        chip.setObjectName("OptionChip")
        chip.setCheckable(True)
        chip.setChecked(checked)
        chip.setMinimumHeight(28)
        return chip

    def _apply_local_style(self, theme: str | None = None):
        theme = theme or load_settings().get("theme", "dark")
        if theme == "light":
            card_bg = "#ffffff"
            card_border = "#d9dee6"
            title_color = "#172033"
            field_color = "#5f6b7a"
            hint_color = "#7a8797"
            chip_bg = "#ffffff"
            chip_border = "#cfd7e3"
            chip_color = "#172033"
            chip_hover_bg = "#f3f7fb"
            chip_hover_border = "#b8c6d8"
            chip_checked_bg = "#e8f2ff"
            chip_checked_border = "#2f80ed"
            chip_checked_color = "#12345a"
            chip_disabled_bg = "#f3f5f8"
            chip_disabled_border = "#dce2ea"
            chip_disabled_color = "#9aa5b3"
        else:
            card_bg = "#242932"
            card_border = "#343d49"
            title_color = "#e8edf4"
            field_color = "#aeb8c5"
            hint_color = "#8f99a6"
            chip_bg = "#2b313a"
            chip_border = "#3b4654"
            chip_color = "#d2dbe6"
            chip_hover_bg = "#323b46"
            chip_hover_border = "#526274"
            chip_checked_bg = "#213c5f"
            chip_checked_border = "#3b8cff"
            chip_checked_color = "#edf6ff"
            chip_disabled_bg = "#24282e"
            chip_disabled_border = "#323942"
            chip_disabled_color = "#717b87"

        self.setStyleSheet(f"""
            QLabel#DialogTitle {{
                font-size: 22px;
                font-weight: 700;
            }}
            QLabel#SectionTitle {{
                font-size: 15px;
                font-weight: 700;
            }}
            QLabel#SectionHint {{
                color: {hint_color};
            }}
            QFrame#TableToolbar {{
                background: transparent;
                border: none;
            }}
            QFrame#SidebarPanel, QFrame#MainPanel {{
                background-color: transparent;
                border: none;
            }}
            QFrame#SettingsCard {{
                background-color: {card_bg};
                border: 1px solid {card_border};
                border-radius: 8px;
            }}
            QLabel#CardTitle {{
                font-size: 15px;
                font-weight: 700;
                color: {title_color};
            }}
            QLabel#FieldLabel {{
                color: {field_color};
            }}
            QWidget#InlineOptions, QFrame#InlineOptions {{
                background: transparent;
                border: none;
            }}
            QPushButton#OptionChip {{
                background-color: {chip_bg};
                border: 1px solid {chip_border};
                border-radius: 8px;
                color: {chip_color};
                padding: 5px 10px;
                text-align: center;
                font-weight: 500;
            }}
            QPushButton#OptionChip:hover {{
                background-color: {chip_hover_bg};
                border-color: {chip_hover_border};
            }}
            QPushButton#OptionChip:checked {{
                background-color: {chip_checked_bg};
                border-color: {chip_checked_border};
                color: {chip_checked_color};
            }}
            QPushButton#OptionChip:disabled {{
                background-color: {chip_disabled_bg};
                border-color: {chip_disabled_border};
                color: {chip_disabled_color};
            }}
            QScrollArea#SettingsScroll {{
                background: transparent;
                border: none;
            }}
        """)

    def refresh_theme(self, theme: str):
        self._apply_local_style(theme)

    def _build_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 14, 16, 14)
        layout.setSpacing(12)

        self._apply_local_style()

        header_row = QHBoxLayout()
        header_row.setContentsMargins(0, 0, 0, 0)
        title_stack = QVBoxLayout()
        title_stack.setContentsMargins(0, 0, 0, 0)
        title_stack.setSpacing(2)
        title = QLabel(self._tr("title"))
        title.setObjectName("DialogTitle")
        subtitle = QLabel(self._tr("subtitle"))
        subtitle.setObjectName("SectionHint")
        title_stack.addWidget(title)
        title_stack.addWidget(subtitle)
        header_row.addLayout(title_stack)
        header_row.addStretch()
        layout.addLayout(header_row)

        splitter = QSplitter(Qt.Orientation.Horizontal)
        splitter.setChildrenCollapsible(False)
        layout.addWidget(splitter, 1)

        sidebar_scroll = QScrollArea()
        sidebar_scroll.setObjectName("SettingsScroll")
        sidebar_scroll.setWidgetResizable(True)
        sidebar_scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        sidebar_scroll.setMinimumWidth(360)
        sidebar_scroll.setMaximumWidth(430)

        sidebar = QFrame()
        sidebar.setObjectName("SidebarPanel")
        sidebar_layout = QVBoxLayout(sidebar)
        sidebar_layout.setContentsMargins(0, 0, 10, 0)
        sidebar_layout.setSpacing(12)

        # ---- Primary workflow: choose corpus range, filter, query ----
        scope_box = QFrame()
        scope_box.setObjectName("SettingsCard")
        scope_box.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
        sg = QVBoxLayout(scope_box)
        sg.setContentsMargins(16, 16, 16, 16)
        sg.setSpacing(11)
        scope_title = QLabel(self._tr("scope"))
        scope_title.setObjectName("CardTitle")
        sg.addWidget(scope_title)

        self.ex_artist = MultiSelectComboBox(self._tr("all_artists"))
        self._fill_artists()
        self.ex_song = MultiSelectComboBox(self._tr("all_songs"))
        self.ex_artist.selectionChanged.connect(self._update_song_list)
        self._update_song_list()

        artist_label = QLabel(self._tr("artist"))
        artist_label.setObjectName("FieldLabel")
        sg.addWidget(artist_label)
        sg.addWidget(self.ex_artist)
        song_label = QLabel(self._tr("song"))
        song_label.setObjectName("FieldLabel")
        sg.addWidget(song_label)
        sg.addWidget(self.ex_song)

        self.ex_pos_noun = self._make_chip(self._tr("noun"))
        self.ex_pos_verb = self._make_chip(self._tr("verb"))
        self.ex_pos_adj  = self._make_chip(self._tr("adj"))
        self.ex_pos_adv  = self._make_chip(self._tr("adv"))
        pos_grid = QGridLayout()
        pos_grid.setContentsMargins(0, 0, 0, 0)
        pos_grid.setHorizontalSpacing(12)
        pos_grid.setVerticalSpacing(6)
        for idx, _chk in enumerate((self.ex_pos_noun, self.ex_pos_verb, self.ex_pos_adj, self.ex_pos_adv)):
            _chk.setChecked(True)
            pos_grid.addWidget(_chk, idx // 2, idx % 2)
        pos_w = QWidget()
        pos_w.setObjectName("InlineOptions")
        pos_w.setLayout(pos_grid)
        pos_label = QLabel(self._tr("pos"))
        pos_label.setObjectName("FieldLabel")
        sg.addWidget(pos_label)
        sg.addWidget(pos_w)

        self.ex_min_freq = QSpinBox()
        self.ex_min_freq.setRange(1, 100)
        self.ex_min_freq.setValue(2)
        self.ex_min_freq.setSuffix(" 回")
        freq_row = QHBoxLayout()
        freq_row.setContentsMargins(0, 0, 0, 0)
        freq_label = QLabel(self._tr("min_freq"))
        freq_label.setObjectName("FieldLabel")
        freq_row.addWidget(freq_label)
        freq_row.addWidget(self.ex_min_freq, 1)
        sg.addLayout(freq_row)

        self.ex_jp_only = self._make_chip(self._tr("jp_only"))
        self.ex_skip_single_kana = self._make_chip(self._tr("skip_kana"))
        self.ex_skip_single_kana.setToolTip("过滤掉只有一个假名字符的词（如 お、が 等）")
        filter_row = QVBoxLayout()
        filter_row.setContentsMargins(0, 0, 0, 0)
        filter_row.setSpacing(6)
        filter_row.addWidget(self.ex_jp_only)
        filter_row.addWidget(self.ex_skip_single_kana)
        filter_w = QWidget()
        filter_w.setObjectName("InlineOptions")
        filter_w.setLayout(filter_row)
        filter_label = QLabel(self._tr("filter"))
        filter_label.setObjectName("FieldLabel")
        sg.addWidget(filter_label)
        sg.addWidget(filter_w)

        self.btn_query = QPushButton(self._tr("query"))
        set_button_role(self.btn_query, "primary")
        self.btn_query.setMinimumHeight(34)
        self.btn_query.clicked.connect(self._load_words)
        sg.addWidget(self.btn_query)
        sidebar_layout.addWidget(scope_box)

        # ---- Anki + JLPT: always visible, compact ----
        anki_box = QFrame()
        anki_box.setObjectName("SettingsCard")
        anki_box.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
        ag = QVBoxLayout(anki_box)
        ag.setContentsMargins(16, 16, 16, 16)
        ag.setSpacing(11)
        anki_title = QLabel(self._tr("anki"))
        anki_title.setObjectName("CardTitle")
        ag.addWidget(anki_title)

        deck_row = QHBoxLayout()
        deck_row.setContentsMargins(0, 0, 0, 0)
        deck_row.setSpacing(8)
        self.deck_combo = QComboBox()
        self.deck_combo.setEditable(True)
        btn_refresh = QPushButton("↻")
        set_button_role(btn_refresh, "subtle")
        btn_refresh.setFixedWidth(34)
        btn_refresh.setToolTip("デッキ一覧を再取得")
        btn_refresh.clicked.connect(self._fetch_anki_info)
        deck_row.addWidget(self.deck_combo, 1)
        deck_row.addWidget(btn_refresh)
        deck_label = QLabel(self._tr("deck"))
        deck_label.setObjectName("FieldLabel")
        ag.addWidget(deck_label)
        ag.addLayout(deck_row)

        import shutil as _sh
        self.chk_fetch_def = self._make_chip(self._tr("fetch_def"))
        self.def_lang_combo = QComboBox()
        self.def_lang_combo.addItems(["中文", "英文"])
        self.def_lang_combo.setFixedWidth(68)
        self.chk_fetch_def.toggled.connect(self.def_lang_combo.setEnabled)
        self.chk_clip_audio = self._make_chip(self._tr("clip_audio"))
        try:
            from project_paths import app_dir
            _local_ff = (app_dir() / "ffmpeg.exe").exists()
        except Exception:
            _local_ff = (pathlib.Path(__file__).resolve().parents[1] / "ffmpeg.exe").exists()
        _has_ff = bool(_sh.which("ffmpeg")) or _local_ff
        self.chk_clip_audio.setChecked(_has_ff)
        self.chk_clip_audio.setEnabled(_has_ff)
        if not _has_ff:
            self.chk_clip_audio.setToolTip("ffmpeg が PATH に見つかりません")
        self.max_ex_spin = QSpinBox()
        self.max_ex_spin.setRange(1, 5)
        self.max_ex_spin.setValue(2)

        content_row = QGridLayout()
        content_row.setContentsMargins(0, 0, 0, 0)
        content_row.setHorizontalSpacing(10)
        content_row.setVerticalSpacing(8)
        content_row.addWidget(self.chk_fetch_def, 0, 0)
        content_row.addWidget(self.def_lang_combo, 0, 1)
        content_row.addWidget(self.chk_clip_audio, 1, 0)
        content_row.addWidget(QLabel(self._tr("examples")), 1, 1)
        content_row.addWidget(self.max_ex_spin, 1, 2)
        content_w = QWidget()
        content_w.setObjectName("InlineOptions")
        content_w.setLayout(content_row)
        content_label = QLabel(self._tr("card_content"))
        content_label.setObjectName("FieldLabel")
        ag.addWidget(content_label)
        ag.addWidget(content_w)

        self.anki_status = QLabel(self._tr("checking"))
        self.anki_status.setWordWrap(True)
        status_label = QLabel(self._tr("status"))
        status_label.setObjectName("FieldLabel")
        ag.addWidget(status_label)
        ag.addWidget(self.anki_status)
        sidebar_layout.addWidget(anki_box)

        learning_box = QFrame()
        learning_box.setObjectName("SettingsCard")
        lg = QVBoxLayout(learning_box)
        lg.setContentsMargins(16, 16, 16, 16)
        lg.setSpacing(8)
        learning_title = QLabel(self._tr("learning"))
        learning_title.setObjectName("CardTitle")
        lg.addWidget(learning_title)

        learning_row = QHBoxLayout()
        learning_row.setContentsMargins(0, 0, 0, 0)
        learning_row.setSpacing(8)
        self.lbl_learning_status = QLabel(self._tr("not_synced"))
        self.lbl_learning_status.setToolTip(
            "读取 Anki collection.anki2 中 JPOP Corpus 笔记的复习次数。\n"
            "已学 = 至少一张对应卡片 reps > 0。")
        self.btn_sync_learning = QPushButton(self._tr("sync"))
        set_button_role(self.btn_sync_learning, "subtle")
        self.btn_sync_learning.setFixedWidth(58)
        self.btn_sync_learning.setToolTip("重新读取 Anki 学习状态并标记词表")
        self.btn_sync_learning.clicked.connect(self._start_learning_sync)
        self.chk_auto_uncheck_studied = self._make_chip(self._tr("auto_uncheck"))
        self.chk_auto_uncheck_studied.setToolTip(
            "同步完成后自动取消已学词的勾选，避免重复导出。\n"
            "已导出但未复习的词只标记，不会自动取消。")
        self.chk_auto_uncheck_studied.toggled.connect(
            lambda checked: self._apply_learning_status(auto_uncheck=checked))
        learning_row.addWidget(self.lbl_learning_status, 1)
        learning_row.addWidget(self.btn_sync_learning)
        lg.addLayout(learning_row)
        lg.addWidget(self.chk_auto_uncheck_studied)
        sidebar_layout.addWidget(learning_box)

        jlpt_box = QFrame()
        jlpt_box.setObjectName("SettingsCard")
        jg = QGridLayout(jlpt_box)
        jg.setContentsMargins(16, 16, 16, 16)
        jg.setHorizontalSpacing(12)
        jg.setVerticalSpacing(8)
        jlpt_title = QLabel("JLPT")
        jlpt_title.setObjectName("CardTitle")
        jg.addWidget(jlpt_title, 0, 0, 1, 3)
        self._jlpt_chks: dict = {}
        for idx, lvl in enumerate(["N5", "N4", "N3", "N2", "N1", "なし"]):
            chk = self._make_chip(lvl)
            chk.toggled.connect(lambda _checked: self._apply_jlpt_filter())
            jg.addWidget(chk, 1 + idx // 3, idx % 3)
            key = lvl if lvl != "なし" else ""
            self._jlpt_chks[key] = chk
        self.lbl_jlpt_fetch = QLabel("")
        jg.addWidget(self.lbl_jlpt_fetch, 3, 0, 1, 3)
        sidebar_layout.addWidget(jlpt_box)

        # ---- Advanced options: visible on demand, but still fully functional ----
        self.btn_advanced = QPushButton("▸ " + self._tr("advanced"))
        set_button_role(self.btn_advanced, "subtle")
        self.btn_advanced.setCheckable(True)
        self.btn_advanced.setToolTip("展开后可调整重复词策略、排序规则和屏蔽词表")
        self.btn_advanced.toggled.connect(self._toggle_advanced_options)
        sidebar_layout.addWidget(self.btn_advanced)

        self.advanced_panel = QWidget()
        adv = QVBoxLayout(self.advanced_panel)
        adv.setContentsMargins(0, 0, 0, 0)
        adv.setSpacing(10)

        repeat_box = QFrame()
        repeat_box.setObjectName("SettingsCard")
        rg = QFormLayout(repeat_box)
        rg.setContentsMargins(16, 16, 16, 16)
        repeat_title = QLabel(self._tr("repeat"))
        repeat_title.setObjectName("CardTitle")
        rg.addRow(repeat_title)

        self.dup_mode_combo = QComboBox()
        self.dup_mode_combo.addItems(["手动选择", "自动追加", "跳过"] if self.lang == "zh" else ["手動選択", "自動追加", "スキップ"])
        self.dup_mode_combo.setToolTip(
            "重复词处理：\n手动选择=弹窗让你决定追加哪些例句\n自动追加=全部新例句自动加入\n跳过=不做修改")
        self.dup_scope_combo = QComboBox()
        self.dup_scope_combo.addItems([self._tr("current_deck"), self._tr("main_deck")])
        self.dup_scope_combo.setToolTip(
            "重复检测范围：\n当前牌组=只在导出目标牌组内检查是否已有该词\n"
            "主牌组=在顶层牌组（及所有子牌组）内检查，防止跨子牌组重复导出")
        self.blacklist_edit = QLineEdit()
        self.blacklist_edit.setPlaceholderText("例：は,が,を,に,の,で,と,も,へ,か,な,ね")
        self.blacklist_edit.setToolTip(
            "查询时过滤掉这些词（助词、感叹词等功能词）\n逗号分隔，支持任意词形")
        rg.addRow(self._tr("duplicate"), self.dup_mode_combo)
        rg.addRow(self._tr("scope_dup"), self.dup_scope_combo)
        rg.addRow(self._tr("blacklist"), self.blacklist_edit)
        adv.addWidget(repeat_box)

        sort_box = QFrame()
        sort_box.setObjectName("SettingsCard")
        sort_grid = QGridLayout(sort_box)
        sort_grid.setContentsMargins(16, 16, 16, 16)
        sort_grid.setHorizontalSpacing(8)
        sort_grid.setVerticalSpacing(8)
        sort_grid.setColumnStretch(1, 1)
        sort_title = QLabel(self._tr("sort"))
        sort_title.setObjectName("CardTitle")
        sort_grid.addWidget(sort_title, 0, 0, 1, 2)

        _sort_keys = [
            ("JLPT 级别",   "jlpt"),
            ("语料库频率",  "corpus"),
            ("JPDB 词频",   "jpdb"),
            ("歌曲",        "song"),
            ("不排序",      "none"),
        ]
        _jlpt_dirs   = ["N5→N1（基础优先）", "N1→N5（进阶优先）"]
        _freq_dirs   = ["多→少（高频优先）",  "少→多（低频优先）"]
        _song_dirs   = ["名称 A→Z",           "名称 Z→A"]
        _none_dirs   = ["—"]
        _dir_map     = {"jlpt": _jlpt_dirs, "corpus": _freq_dirs,
                        "jpdb": _freq_dirs,  "song": _song_dirs, "none": _none_dirs}

        def _make_sort_pair(default_key="jlpt"):
            key_cb  = QComboBox()
            dir_cb  = QComboBox()
            for lbl, val in _sort_keys:
                key_cb.addItem(lbl, val)
            key_cb.setCurrentIndex(
                next(i for i, (_, v) in enumerate(_sort_keys) if v == default_key))
            dir_cb.addItems(_dir_map[default_key])
            def _on_key_changed(idx, _key_cb=key_cb, _dir_cb=dir_cb):
                k = _key_cb.currentData()
                _dir_cb.clear()
                _dir_cb.addItems(_dir_map.get(k, ["—"]))
            key_cb.currentIndexChanged.connect(_on_key_changed)
            return key_cb, dir_cb

        main_sort_label = QLabel(self._tr("primary_sort"))
        main_sort_label.setObjectName("FieldLabel")
        sort_grid.addWidget(main_sort_label, 1, 0)
        self.sort_key1, self.sort_dir1 = _make_sort_pair("jlpt")
        sort_grid.addWidget(self.sort_key1, 1, 1)
        sort_grid.addWidget(self.sort_dir1, 2, 1)

        sub_sort_label = QLabel(self._tr("secondary_sort"))
        sub_sort_label.setObjectName("FieldLabel")
        sort_grid.addWidget(sub_sort_label, 3, 0)
        self.sort_key2, self.sort_dir2 = _make_sort_pair("corpus")
        sort_grid.addWidget(self.sort_key2, 3, 1)
        sort_grid.addWidget(self.sort_dir2, 4, 1)

        no_jlpt_label = QLabel(self._tr("no_jlpt"))
        no_jlpt_label.setObjectName("FieldLabel")
        sort_grid.addWidget(no_jlpt_label, 5, 0)
        self.sort_no_jlpt = QComboBox()
        self.sort_no_jlpt.addItems([
            "按JPDB词频估算", "按语料库频率估算", "放末尾（不估算）"])
        self.sort_no_jlpt.setToolTip(
            "对没有JLPT级别的词：\n"
            "按JPDB词频估算 = 用JPDB排名推断难度\n"
            "按语料库频率估算 = 出现越多视为越基础\n"
            "放末尾 = 归入最难/最后")
        sort_grid.addWidget(self.sort_no_jlpt, 5, 1)

        self.chk_group_song = self._make_chip(self._tr("group_song"))
        self.chk_group_song.setToolTip(
            "勾选后先按歌曲聚合，每首歌内部再按主/次排序，\n"
            "适合批量导入某首歌的词汇时保持上下文连贯")
        sort_grid.addWidget(self.chk_group_song, 6, 0, 1, 2)

        btn_sort = QPushButton(self._tr("apply_sort"))
        set_button_role(btn_sort, "subtle")
        btn_sort.clicked.connect(self._apply_sort)
        sort_grid.addWidget(btn_sort, 7, 0, 1, 2)
        adv.addWidget(sort_box)
        self.advanced_panel.setVisible(False)
        sidebar_layout.addWidget(self.advanced_panel)
        sidebar_layout.addStretch()

        sidebar_scroll.setWidget(sidebar)
        splitter.addWidget(sidebar_scroll)

        main_panel = QFrame()
        main_panel.setObjectName("MainPanel")
        main_layout = QVBoxLayout(main_panel)
        main_layout.setContentsMargins(10, 0, 0, 0)
        main_layout.setSpacing(10)

        table_toolbar = QFrame()
        table_toolbar.setObjectName("TableToolbar")
        table_toolbar_layout = QHBoxLayout(table_toolbar)
        table_toolbar_layout.setContentsMargins(0, 0, 0, 0)
        table_toolbar_layout.setSpacing(8)
        table_title = QLabel(self._tr("word_list"))
        table_title.setObjectName("SectionTitle")
        table_hint = QLabel(self._tr("word_hint"))
        table_hint.setObjectName("SectionHint")
        table_toolbar_layout.addWidget(table_title)
        table_toolbar_layout.addWidget(table_hint)
        table_toolbar_layout.addStretch()
        main_layout.addWidget(table_toolbar)

        self.word_table = QTableWidget()
        self.word_table.setColumnCount(8)
        self.word_table.setHorizontalHeaderLabels(
            ["", self._tr("lemma"), self._tr("table_pos"), "JLPT", self._tr("occurrences"),
             self._tr("song_count"), self._tr("jpdb"), self._tr("anki_status")])
        wh = self.word_table.horizontalHeader()
        wh.setSectionResizeMode(0, QHeaderView.ResizeMode.ResizeToContents)
        wh.setSectionResizeMode(1, QHeaderView.ResizeMode.Stretch)
        wh.setSectionResizeMode(2, QHeaderView.ResizeMode.ResizeToContents)
        wh.setSectionResizeMode(3, QHeaderView.ResizeMode.ResizeToContents)
        wh.setSectionResizeMode(4, QHeaderView.ResizeMode.ResizeToContents)
        wh.setSectionResizeMode(5, QHeaderView.ResizeMode.ResizeToContents)
        wh.setSectionResizeMode(6, QHeaderView.ResizeMode.ResizeToContents)
        wh.setSectionResizeMode(7, QHeaderView.ResizeMode.ResizeToContents)
        self.word_table.setSelectionBehavior(QTableWidget.SelectionBehavior.SelectRows)
        self.word_table.setEditTriggers(QTableWidget.EditTrigger.NoEditTriggers)
        tune_table(self.word_table, 32)
        self.word_table.itemChanged.connect(self._update_sel_count)
        main_layout.addWidget(self.word_table, 1)

        action_panel = QFrame()
        action_panel.setObjectName("ActionPanel")
        action_panel.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
        action_layout = QVBoxLayout(action_panel)
        action_layout.setContentsMargins(12, 10, 12, 10)
        action_layout.setSpacing(8)

        select_row = QHBoxLayout()
        select_row.setContentsMargins(0, 0, 0, 0)
        select_row.setSpacing(8)
        self.lbl_sel = QLabel(self._tr("selected", n=0))
        select_row.addWidget(self.lbl_sel)
        btn_all  = QPushButton(self._tr("select_all"))
        set_button_role(btn_all, "subtle")
        btn_all.setFixedWidth(72)
        btn_all.clicked.connect(lambda: self._set_all_checked(True))
        btn_none = QPushButton(self._tr("clear"))
        set_button_role(btn_none, "subtle")
        btn_none.setFixedWidth(72)
        btn_none.clicked.connect(lambda: self._set_all_checked(False))
        select_row.addWidget(btn_all)
        select_row.addWidget(btn_none)
        select_row.addStretch()
        btn_close = QPushButton(self._tr("close"))
        set_button_role(btn_close, "subtle")
        btn_close.setFixedWidth(84)
        btn_close.clicked.connect(self.accept)
        select_row.addWidget(btn_close)
        action_layout.addLayout(select_row)

        command_row = QHBoxLayout()
        command_row.setContentsMargins(0, 0, 0, 0)
        command_row.setSpacing(8)

        self.btn_export = QPushButton(self._tr("send"))
        set_button_role(self.btn_export, "primary")
        self.btn_export.setMinimumWidth(142)
        self.btn_export.setEnabled(False)
        self.btn_export.clicked.connect(self._start_export)
        self.btn_update = QPushButton(self._tr("update"))
        set_button_role(self.btn_update, "subtle")
        self.btn_update.setMinimumWidth(116)
        self.btn_update.setEnabled(False)
        self.btn_update.setToolTip(
            "重新获取释义/读音/声调/词频，覆盖 Anki 中已有卡片的对应字段\n"
            "（不改动已有例句；同时刷新卡片模板 CSS）")
        self.btn_update.clicked.connect(self._start_update)
        self.refresh_scope_combo = QComboBox()
        self.refresh_scope_combo.addItems([self._tr("current_only"), self._tr("current_children"), self._tr("main_children")])
        self.refresh_scope_combo.setMinimumWidth(150)
        self.refresh_scope_combo.setToolTip(
            "刷新旧牌组的范围：\n"
            "仅当前牌组=只刷新下拉框中选中的这个次牌组\n"
            "当前牌组+子牌组=刷新该牌组及其下级\n"
            "主牌组+子牌组=刷新顶层主牌组下全部子牌组")
        self.btn_refresh_deck = QPushButton(self._tr("refresh_deck"))
        set_button_role(self.btn_refresh_deck, "subtle")
        self.btn_refresh_deck.setMinimumWidth(116)
        self.btn_refresh_deck.setEnabled(False)
        self.btn_refresh_deck.setToolTip(
            "扫描当前牌组中的全部 JPOP Corpus 旧卡，批量刷新新版中文释义、词性、JLPT、声调、JPDB 词频和模板。\n"
            "不改动已有例句和音频。")
        self.btn_refresh_deck.clicked.connect(self._start_refresh_deck)

        command_row.addWidget(self.btn_export)
        command_row.addWidget(self.btn_update)
        command_row.addStretch()
        command_row.addWidget(QLabel(self._tr("refresh_scope")))
        command_row.addWidget(self.refresh_scope_combo)
        command_row.addWidget(self.btn_refresh_deck)
        action_layout.addLayout(command_row)

        progress_row = QHBoxLayout()
        progress_row.setContentsMargins(0, 0, 0, 0)
        self.progress_bar = QProgressBar()
        self.progress_bar.setVisible(False)
        self.lbl_prog = QLabel("")
        progress_row.addWidget(self.progress_bar, 1)
        progress_row.addWidget(self.lbl_prog)
        action_layout.addLayout(progress_row)
        main_layout.addWidget(action_panel)

        self.log_list = QListWidget()
        self.log_list.setFixedHeight(120)
        self.log_list.setVisible(False)
        self.log_list.setStyleSheet(
            "font-size:12px; font-family:'Consolas','Courier New',monospace;")
        main_layout.addWidget(self.log_list)

        splitter.addWidget(main_panel)
        splitter.setSizes([390, 890])


    def _toggle_advanced_options(self, checked: bool):
        self.advanced_panel.setVisible(checked)
        self.btn_advanced.setText(
            ("▾ " if checked else "▸ ") + self._tr("advanced")
        )

    # ---- Data loaders ----

    def _fill_artists(self):
        try:
            conn = sqlite3.connect(str(DB_PATH))
            counts: dict[str, int] = {}
            for (a,) in conn.execute("SELECT artist FROM songs").fetchall():
                for part in a.split("/"):
                    part = part.strip()
                    if part:
                        counts[part] = counts.get(part, 0) + 1
            conn.close()
        except Exception:
            counts = {}
        labeled = [
            (f"{n} ({c}首)", n)
            for n, c in sorted(counts.items(), key=lambda x: (-x[1], x[0]))
        ]
        self.ex_artist.setItemsLabeled(labeled)

    def _update_song_list(self):
        artists = self.ex_artist.selectedValues()
        try:
            conn = sqlite3.connect(str(DB_PATH))
            if artists:
                conds = " OR ".join(
                    ["'/' || artist || '/' LIKE '%/' || ? || '/%'"] * len(artists)
                )
                rows = conn.execute(
                    f"SELECT id, title, artist FROM songs WHERE {conds}"
                    " ORDER BY artist, title", artists
                ).fetchall()
            else:
                rows = conn.execute(
                    "SELECT id, title, artist FROM songs ORDER BY artist, title"
                ).fetchall()
            conn.close()
        except Exception:
            rows = []
        labeled = [(f"{title}  [{artist}]", sid)
                   for sid, title, artist in rows]
        self.ex_song.setItemsLabeled(labeled)

    def _restore_anki_settings(self):
        s = load_settings().get("anki_dialog", {})
        if "fetch_def" in s:
            self.chk_fetch_def.setChecked(s["fetch_def"])
        if "def_lang" in s:
            idx = self.def_lang_combo.findText(s["def_lang"][:2], Qt.MatchFlag.MatchContains)
            if idx >= 0:
                self.def_lang_combo.setCurrentIndex(idx)
        if "clip_audio" in s and self.chk_clip_audio.isEnabled():
            self.chk_clip_audio.setChecked(s["clip_audio"])
        if "max_examples" in s:
            self.max_ex_spin.setValue(s["max_examples"])
        if "jp_only" in s:
            self.ex_jp_only.setChecked(s["jp_only"])
        if "skip_single_kana" in s:
            self.ex_skip_single_kana.setChecked(s["skip_single_kana"])
        if "auto_uncheck_studied" in s:
            self.chk_auto_uncheck_studied.setChecked(s["auto_uncheck_studied"])
        if "word_blacklist" in s:
            self.blacklist_edit.setText(s["word_blacklist"])
        if "pos_noun" in s: self.ex_pos_noun.setChecked(s["pos_noun"])
        if "pos_verb" in s: self.ex_pos_verb.setChecked(s["pos_verb"])
        if "pos_adj"  in s: self.ex_pos_adj.setChecked(s["pos_adj"])
        if "pos_adv"  in s: self.ex_pos_adv.setChecked(s["pos_adv"])
        if "dup_mode_index" in s:
            self.dup_mode_combo.setCurrentIndex(min(s["dup_mode_index"], self.dup_mode_combo.count() - 1))
        elif "dup_mode" in s:
            idx = self.dup_mode_combo.findText(s["dup_mode"])
            if idx >= 0:
                self.dup_mode_combo.setCurrentIndex(idx)
        if "dup_scope_index" in s:
            self.dup_scope_combo.setCurrentIndex(min(s["dup_scope_index"], self.dup_scope_combo.count() - 1))
        elif "dup_scope" in s:
            idx = self.dup_scope_combo.findText(s["dup_scope"])
            if idx >= 0:
                self.dup_scope_combo.setCurrentIndex(idx)
        if "refresh_scope_index" in s:
            self.refresh_scope_combo.setCurrentIndex(min(s["refresh_scope_index"], self.refresh_scope_combo.count() - 1))
        elif "refresh_scope" in s:
            idx = self.refresh_scope_combo.findText(s["refresh_scope"])
            if idx >= 0:
                self.refresh_scope_combo.setCurrentIndex(idx)
        # Sort settings
        sort = s.get("sort", {})
        if "key1" in sort:
            idx = self.sort_key1.findData(sort["key1"])
            if idx >= 0: self.sort_key1.setCurrentIndex(idx)
        if "dir1" in sort:
            idx = self.sort_dir1.findText(sort["dir1"])
            if idx >= 0: self.sort_dir1.setCurrentIndex(idx)
        if "key2" in sort:
            idx = self.sort_key2.findData(sort["key2"])
            if idx >= 0: self.sort_key2.setCurrentIndex(idx)
        if "dir2" in sort:
            idx = self.sort_dir2.findText(sort["dir2"])
            if idx >= 0: self.sort_dir2.setCurrentIndex(idx)
        if "no_jlpt" in sort:
            self.sort_no_jlpt.setCurrentIndex(sort["no_jlpt"])
        if "group_song" in sort:
            self.chk_group_song.setChecked(sort["group_song"])
        # Connect save signals
        for w in (self.chk_fetch_def, self.chk_clip_audio, self.ex_jp_only,
                  self.ex_skip_single_kana, self.chk_group_song,
                  self.chk_auto_uncheck_studied, self.ex_pos_noun,
                  self.ex_pos_verb, self.ex_pos_adj, self.ex_pos_adv):
            w.toggled.connect(self._save_anki_settings)
        for cb in (self.def_lang_combo, self.dup_mode_combo, self.dup_scope_combo,
                   self.refresh_scope_combo, self.sort_key1, self.sort_dir1,
                   self.sort_key2, self.sort_dir2, self.sort_no_jlpt):
            cb.currentIndexChanged.connect(self._save_anki_settings)
        self.max_ex_spin.valueChanged.connect(self._save_anki_settings)
        self.deck_combo.currentTextChanged.connect(self._save_anki_settings)
        self.blacklist_edit.textChanged.connect(self._save_anki_settings)

    def _save_anki_settings(self, *_):
        s = load_settings()
        s["anki_dialog"] = {
            "deck":         self.deck_combo.currentText(),
            "fetch_def":    self.chk_fetch_def.isChecked(),
            "def_lang":     self.def_lang_combo.currentText(),
            "clip_audio":   self.chk_clip_audio.isChecked(),
            "max_examples": self.max_ex_spin.value(),
            "jp_only":          self.ex_jp_only.isChecked(),
            "skip_single_kana": self.ex_skip_single_kana.isChecked(),
            "auto_uncheck_studied": self.chk_auto_uncheck_studied.isChecked(),
            "word_blacklist":    self.blacklist_edit.text(),
            "pos_noun": self.ex_pos_noun.isChecked(),
            "pos_verb": self.ex_pos_verb.isChecked(),
            "pos_adj":  self.ex_pos_adj.isChecked(),
            "pos_adv":  self.ex_pos_adv.isChecked(),
            "dup_mode":         self.dup_mode_combo.currentText(),
            "dup_mode_index":   self.dup_mode_combo.currentIndex(),
            "dup_scope":        self.dup_scope_combo.currentText(),
            "dup_scope_index":  self.dup_scope_combo.currentIndex(),
            "refresh_scope":    self.refresh_scope_combo.currentText(),
            "refresh_scope_index": self.refresh_scope_combo.currentIndex(),
            "sort": {
                "key1":       self.sort_key1.currentData(),
                "dir1":       self.sort_dir1.currentText(),
                "key2":       self.sort_key2.currentData(),
                "dir2":       self.sort_dir2.currentText(),
                "no_jlpt":    self.sort_no_jlpt.currentIndex(),
                "group_song": self.chk_group_song.isChecked(),
            },
        }
        save_settings(s)

    def _fetch_anki_info(self):
        """Fetch AnkiConnect deck list in a background thread to avoid blocking the UI."""
        self.anki_status.setText(self._tr("checking"))

        class _AnkiInfoWorker(QThread):
            done = pyqtSignal(list, str)   # decks, error_msg
            def run(self):
                try:
                    decks = _anki_request("deckNames") or []
                    self.done.emit(sorted(decks), "")
                except Exception as e:
                    self.done.emit([], str(e))

        def _on_done(decks, err):
            if err:
                self.anki_status.setText(f"❌ {err}")
            else:
                self.deck_combo.blockSignals(True)
                self.deck_combo.clear()
                for d in decks:
                    self.deck_combo.addItem(d)
                last_deck = load_settings().get("anki_dialog", {}).get("deck", "")
                if last_deck:
                    idx = self.deck_combo.findText(last_deck)
                    if idx >= 0:
                        self.deck_combo.setCurrentIndex(idx)
                    else:
                        self.deck_combo.setEditText(last_deck)
                self.deck_combo.blockSignals(False)
                self.anki_status.setText(f"✅ 接続 OK — {len(decks)} デッキ")
            self._update_sel_count()

        self._anki_info_worker = _AnkiInfoWorker()
        self._anki_info_worker.done.connect(_on_done)
        self._anki_info_worker.start()

    def _start_learning_sync(self, *_):
        """Load Anki review state in the background and mark the word table."""
        if self._learning_worker and self._learning_worker.isRunning():
            return
        self.lbl_learning_status.setText("同步中…")
        self.btn_sync_learning.setEnabled(False)

        self._learning_worker = AnkiLearningSyncWorker()
        self._learning_worker.done.connect(self._on_learning_sync_done)
        self._learning_worker.start()

    def _on_learning_sync_done(self, status_map: dict, collection_path: str, err: str):
        self.btn_sync_learning.setEnabled(True)
        if err:
            self._learning_status = {}
            self._learning_collection_path = ""
            self.lbl_learning_status.setText(f"❌ {err}")
            self.lbl_learning_status.setToolTip("同步失败。确认 Anki 已安装，或先运行一次 Anki。")
            self._apply_learning_status(auto_uncheck=False)
            return

        self._learning_status = status_map or {}
        self._learning_collection_path = collection_path
        total = len(self._learning_status)
        studied = sum(1 for st in self._learning_status.values() if st.studied)
        self.lbl_learning_status.setText(f"✅ 已同步：{studied}/{total} 已学")
        self.lbl_learning_status.setToolTip(f"来源：{collection_path}")
        self._apply_learning_status(
            auto_uncheck=self.chk_auto_uncheck_studied.isChecked())

    def _learning_label_for(self, lemma: str):
        st = self._learning_status.get(lemma)
        if not self._learning_status:
            return "未同步", "#7f8c8d", ""
        if st is None:
            return "未导出", "#7f8c8d", ""
        if st.studied:
            return "已学", "#27ae60", (
                f"已学：{st.note_count} 笔记 / {st.card_count} 卡片 / "
                f"最高 reps={st.max_reps}\n" + "\n".join(sorted(st.decks)))
        return "已导出", "#d39b2a", (
            f"已导出但未复习：{st.note_count} 笔记 / {st.card_count} 卡片\n"
            + "\n".join(sorted(st.decks)))

    def _apply_learning_status(self, auto_uncheck: bool = False):
        if not hasattr(self, "word_table"):
            return
        self.word_table.blockSignals(True)
        for i in range(self.word_table.rowCount()):
            lemma_it = self.word_table.item(i, 1)
            if not lemma_it:
                continue
            lemma = lemma_it.text()
            label, color, tooltip = self._learning_label_for(lemma)
            status_it = self.word_table.item(i, 7) or QTableWidgetItem()
            status_it.setText(label)
            status_it.setTextAlignment(
                Qt.AlignmentFlag.AlignCenter | Qt.AlignmentFlag.AlignVCenter)
            if tooltip:
                status_it.setToolTip(tooltip)
            else:
                status_it.setToolTip("")
            status_it.setForeground(QBrush(QColor(color)))
            self.word_table.setItem(i, 7, status_it)

            st = self._learning_status.get(lemma)
            row_bg = QBrush()
            if st and st.studied:
                row_bg = QBrush(QColor(39, 174, 96, 34))
            elif st and st.exists:
                row_bg = QBrush(QColor(211, 155, 42, 30))
            for col in range(1, self.word_table.columnCount()):
                item = self.word_table.item(i, col)
                if item:
                    item.setBackground(row_bg)

            if auto_uncheck and st and st.studied:
                chk = self.word_table.item(i, 0)
                if chk:
                    chk.setCheckState(Qt.CheckState.Unchecked)
        self.word_table.blockSignals(False)
        self._update_sel_count()

    def _start_jlpt_fetch(self):
        if self._jlpt_worker and self._jlpt_worker.isRunning():
            self._jlpt_worker.requestInterruption()
            self._jlpt_worker.wait(500)
        self._jlpt_map.clear()
        lemmas = [row[0] for row in self._word_rows]
        if not lemmas:
            return

        # ── Sync pre-load from jlpt_cache so filter works immediately ──
        try:
            _ensure_jlpt_cache_table()
            conn = sqlite3.connect(str(DB_PATH))
            ph = ",".join("?" * len(lemmas))
            raw_cached = dict(conn.execute(
                f"SELECT lemma, level FROM jlpt_cache WHERE lemma IN ({ph})",
                lemmas
            ).fetchall())
            conn.close()
        except Exception:
            raw_cached = {}

        # Normalize old "JLPT-N3" format → "N3"
        cached = {
            lm: (lvl.replace("JLPT-", "") if lvl.startswith("JLPT-") else lvl)
            for lm, lvl in raw_cached.items()
        }

        for i, lemma in enumerate(lemmas):
            if lemma in cached:
                level = cached[lemma]
                self._jlpt_map[lemma] = level
                it = self.word_table.item(i, 3)
                if it:
                    it.setText(level if level else "—")

        # Apply filter for already-known levels before background fetch
        self._apply_jlpt_filter()

        # Only fetch uncached lemmas in background
        uncached = [l for l in lemmas if l not in cached]
        n_cached = len(cached)
        self.lbl_jlpt_fetch.setText(
            f"JLPT: {n_cached} キャッシュ済み, {len(uncached)} 取得中…"
            if uncached else f"✅ JLPT 全 {n_cached} 語 キャッシュ済み"
        )
        if not uncached:
            self._apply_sort()   # all JLPT already known — sort now
            return
        self._jlpt_worker = JlptFetchWorker(uncached)
        self._jlpt_worker.result.connect(self._on_jlpt_result)
        self._jlpt_worker.done.connect(self._on_jlpt_done)
        self._jlpt_worker.start()

    def _apply_jlpt_filter(self):
        """Auto-check/uncheck word table rows based on JLPT checkboxes."""
        wanted = {lvl for lvl, chk in self._jlpt_chks.items() if chk.isChecked()}
        all_selected = len(wanted) == len(self._jlpt_chks)
        self.word_table.blockSignals(True)
        for i in range(self.word_table.rowCount()):
            lemma_it = self.word_table.item(i, 1)
            chk_it = self.word_table.item(i, 0)
            if not lemma_it or not chk_it:
                continue
            lemma = lemma_it.text()
            learned = (
                self.chk_auto_uncheck_studied.isChecked() and
                bool(self._learning_status.get(lemma) and self._learning_status[lemma].studied)
            )
            if learned:
                chk_it.setCheckState(Qt.CheckState.Unchecked)
                continue
            if all_selected:
                chk_it.setCheckState(Qt.CheckState.Checked)
                continue
            if lemma not in self._jlpt_map:
                # JLPT unknown → uncheck (conservative: don't include unverified)
                chk_it.setCheckState(Qt.CheckState.Unchecked)
                continue
            level = self._jlpt_map[lemma]  # "" means no JLPT (= "なし")
            state = Qt.CheckState.Checked if level in wanted else Qt.CheckState.Unchecked
            chk_it.setCheckState(state)
        self.word_table.blockSignals(False)
        self._update_sel_count()

    def _on_jlpt_result(self, lemma: str, level: str):
        self._jlpt_map[lemma] = level
        n = len(self._jlpt_map)
        total = len(self._word_rows)
        self.lbl_jlpt_fetch.setText(f"JLPT 取得中… {n}/{total}")
        i = self._row_index.get(lemma, -1)
        if i < 0:
            return
        lvl_it = QTableWidgetItem(level if level else "—")
        lvl_it.setTextAlignment(
            Qt.AlignmentFlag.AlignCenter | Qt.AlignmentFlag.AlignVCenter)
        self.word_table.setItem(i, 3, lvl_it)
        wanted = {lvl for lvl, chk in self._jlpt_chks.items() if chk.isChecked()}
        if len(wanted) < len(self._jlpt_chks):
            chk_it = self.word_table.item(i, 0)
            if chk_it:
                state = Qt.CheckState.Checked if level in wanted \
                        else Qt.CheckState.Unchecked
                st = self._learning_status.get(lemma)
                if (self.chk_auto_uncheck_studied.isChecked() and
                        st and st.studied):
                    state = Qt.CheckState.Unchecked
                chk_it.setCheckState(state)
                self._update_sel_count()

    def _on_jlpt_done(self):
        self.lbl_jlpt_fetch.setText(f"✅ JLPT 取得完了")
        self._apply_sort()   # re-sort now that all JLPT levels are known

    def _ensure_yomitan_bg(self):
        """Build Yomitan cache + download static JLPT in background (first-time only)."""
        global _yomitan_ready
        sett = load_settings()
        zip_path = sett.get("yomitan_zh_dict", "")

        class _BgWorker(QThread):
            done = pyqtSignal(str)   # status message
            def __init__(self, path):
                super().__init__()
                self._path = path
            def run(self):
                global _yomitan_ready
                msgs = []
                # 1. Yomitan cache (Chinese defs + pitch + freq)
                if not _yomitan_ready and self._path and pathlib.Path(self._path).exists():
                    ok = _ensure_yomitan_cache(self._path)
                    _yomitan_ready = ok
                    if ok:
                        msgs.append("📚 本地词典就绪")
                # 2. Static JLPT download (one-time, ~400KB from GitHub)
                try:
                    import sqlite3 as _sq
                    c = _sq.connect(str(DB_PATH))
                    cnt = c.execute("SELECT COUNT(*) FROM jlpt_cache").fetchone()[0]
                    c.close()
                except Exception:
                    cnt = 0
                if cnt < 5000:
                    ok2 = _download_jlpt_static()
                    if ok2:
                        msgs.append("✅ JLPT 词表已下载")
                self.done.emit("  ".join(msgs) if msgs else "")

        self._yomitan_worker = _BgWorker(zip_path)

        def _on_done(msg):
            if msg and hasattr(self, 'lbl_jlpt_fetch'):
                self.lbl_jlpt_fetch.setText(msg)
                QTimer.singleShot(4000, lambda: self.lbl_jlpt_fetch.setText(""))

        self._yomitan_worker.done.connect(_on_done)
        self._yomitan_worker.start()

    def _load_words(self):
        artists  = self.ex_artist.selectedValues()
        song_ids = self.ex_song.selectedValues()
        selected_pos = [p for p, c in [
            ("NOUN", self.ex_pos_noun), ("VERB", self.ex_pos_verb),
            ("ADJ",  self.ex_pos_adj),  ("ADV",  self.ex_pos_adv),
        ] if c.isChecked()] or ["NOUN", "VERB", "ADJ", "ADV"]
        min_freq = self.ex_min_freq.value()
        jp_only  = self.ex_jp_only.isChecked()
        skip_single_kana = self.ex_skip_single_kana.isChecked()

        conds, params = [], []
        if song_ids:
            ph = ",".join("?" * len(song_ids))
            conds.append(f"s.id IN ({ph})")
            params.extend(song_ids)
        elif artists:
            c = " OR ".join(
                ["'/' || s.artist || '/' LIKE '%/' || ? || '/%'"] * len(artists)
            )
            conds.append(f"({c})")
            params.extend(artists)

        ph = ",".join("?" * len(selected_pos))
        conds.append(f"t.pos IN ({ph})")
        params.extend(selected_pos)

        where = "WHERE " + " AND ".join(conds)
        params.append(min_freq)

        try:
            conn = sqlite3.connect(str(DB_PATH))
            rows = conn.execute(f"""
                SELECT t.lemma, t.pos, COUNT(*) AS freq,
                       COUNT(DISTINCT u.song_id) AS scnt
                FROM tokens t
                JOIN utterances u ON u.id = t.utterance_id
                JOIN songs s ON s.id = u.song_id
                {where}
                GROUP BY t.lemma, t.pos
                HAVING freq >= ?
                ORDER BY freq DESC
                LIMIT 500
            """, params).fetchall()
            conn.close()
        except Exception as e:
            QMessageBox.critical(self, "エラー", str(e))
            return

        if jp_only:
            rows = [r for r in rows if _JP_RE.search(r[0])]
        if skip_single_kana:
            rows = [r for r in rows if not re.fullmatch(r'[\u3041-\u309f\u30a0-\u30ff]', r[0])]
        blacklist = {w.strip() for w in self.blacklist_edit.text().split(",") if w.strip()}
        if blacklist:
            rows = [r for r in rows if r[0] not in blacklist]
        self._word_rows = rows
        self._batch_fetch_jpdb_freq(rows)   # one SQL query for all JPDB freqs
        self._populate_word_table(rows)
        self._start_jlpt_fetch()   # will call _apply_sort when done
        self._start_learning_sync()  # mark learned/exported words without blocking UI

    def _batch_fetch_jpdb_freq(self, rows):
        """Batch-load JPDB frequency ranks for all lemmas in one SQL query."""
        if not rows:
            return
        lemmas = list({r[0] for r in rows})
        try:
            if not _check_yomitan_table("yomitan_freq"):
                return
            conn = sqlite3.connect(str(DB_PATH))
            ph = ",".join("?" * len(lemmas))
            self._jpdb_freq_map = {
                term: freq
                for term, freq in conn.execute(
                    f"SELECT term, freq FROM yomitan_freq WHERE term IN ({ph})",
                    lemmas
                ).fetchall()
            }
            conn.close()
        except Exception:
            pass

    def _apply_sort(self):
        if not self._word_rows:
            return

        key1       = self.sort_key1.currentData()
        dir1_idx   = self.sort_dir1.currentIndex()   # 0=asc, 1=desc
        key2       = self.sort_key2.currentData()
        dir2_idx   = self.sort_dir2.currentIndex()
        no_jlpt    = self.sort_no_jlpt.currentIndex()  # 0=jpdb, 1=corpus, 2=end
        group_song = self.chk_group_song.isChecked()

        JLPT_RANK = {"N5": 0, "N4": 1, "N3": 2, "N2": 3, "N1": 4}
        FREQ_TO_RANK = lambda r: (
            0 if r <= 3000 else 1 if r <= 8000 else
            2 if r <= 15000 else 3 if r <= 25000 else
            4 if r <= 40000 else 5)

        # Use pre-fetched JPDB freq map; batch-fetch dominant song per lemma
        jpdb_cache: dict = {
            lemma: (self._jpdb_freq_map.get(lemma) or 999999)
            for lemma, *_ in self._word_rows
        }
        song_cache: dict = {}
        try:
            lemmas = list({r[0] for r in self._word_rows})
            ph = ",".join("?" * len(lemmas))
            conn = sqlite3.connect(str(DB_PATH))
            for lemma, title in conn.execute(
                f"SELECT t.lemma, s.title FROM tokens t"
                f" JOIN utterances u ON u.id=t.utterance_id"
                f" JOIN songs s ON s.id=u.song_id"
                f" WHERE t.lemma IN ({ph})"
                f" GROUP BY t.lemma, s.id ORDER BY t.lemma, COUNT(*) DESC",
                lemmas
            ).fetchall():
                if lemma not in song_cache:   # first row per lemma = most frequent song
                    song_cache[lemma] = title
            conn.close()
        except Exception:
            pass

        def jlpt_sort_val(lemma):
            lvl = self._jlpt_map.get(lemma, "")
            if lvl in JLPT_RANK:
                return (0, JLPT_RANK[lvl])   # known JLPT, bucket 0
            # Unknown JLPT
            if no_jlpt == 0:   # estimate from JPDB
                freq = jpdb_cache.get(lemma, 999999)
                return (0, FREQ_TO_RANK(freq)) if freq < 999999 else (1, 5)
            elif no_jlpt == 1:  # estimate from corpus freq (higher freq = lower rank)
                corpus_freq = next(
                    (r[2] for r in self._word_rows if r[0] == lemma), 0)
                # normalise: treat corpus freq 10+ as N5, <2 as unknown
                if corpus_freq >= 10:   return (0, 0)
                if corpus_freq >= 5:    return (0, 1)
                if corpus_freq >= 3:    return (0, 2)
                return (1, 5)
            else:               # put at end
                return (1, 5)

        def get_val(lemma, corpus_freq, key):
            if key == "jlpt":
                return jlpt_sort_val(lemma)
            elif key == "corpus":
                return corpus_freq
            elif key == "jpdb":
                return jpdb_cache.get(lemma, 999999)
            elif key == "song":
                return song_cache.get(lemma, "")
            return 0

        def sort_key(row):
            lemma, _, corpus_freq, __ = row
            parts = []
            if group_song:
                parts.append(song_cache.get(lemma, ""))

            v1 = get_val(lemma, corpus_freq, key1)
            # For tuple vals (jlpt), negate second element for descending
            if dir1_idx == 1:
                v1 = (-v1[1], v1[0]) if isinstance(v1, tuple) else -v1
            parts.append(v1)

            if key2 != "none":
                v2 = get_val(lemma, corpus_freq, key2)
                if dir2_idx == 1:
                    v2 = (-v2[1], v2[0]) if isinstance(v2, tuple) else -v2
                parts.append(v2)

            return tuple(parts)

        try:
            rows = sorted(self._word_rows, key=sort_key)
        except TypeError:
            # Mixed types (str vs int) — fallback to str comparison
            rows = sorted(self._word_rows,
                          key=lambda r: str(sort_key(r)))
        self._word_rows = rows
        self._populate_word_table(rows)

    def _populate_word_table(self, rows):
        self.word_table.blockSignals(True)
        self.word_table.setRowCount(len(rows))
        for i, (lemma, pos, freq, scnt) in enumerate(rows):
            chk = QTableWidgetItem()
            chk.setFlags(Qt.ItemFlag.ItemIsEnabled | Qt.ItemFlag.ItemIsUserCheckable)
            chk.setCheckState(Qt.CheckState.Checked)
            self.word_table.setItem(i, 0, chk)
            self.word_table.setItem(i, 1, QTableWidgetItem(lemma))
            self.word_table.setItem(i, 2, QTableWidgetItem(_POS_JA.get(pos, pos)))
            jlpt_val = self._jlpt_map.get(lemma)
            jlpt_txt = (jlpt_val if jlpt_val else "—") if jlpt_val is not None else "…"
            jlpt_it = QTableWidgetItem(jlpt_txt)
            jlpt_it.setTextAlignment(Qt.AlignmentFlag.AlignCenter | Qt.AlignmentFlag.AlignVCenter)
            self.word_table.setItem(i, 3, jlpt_it)
            for col, val in [(4, freq), (5, scnt)]:
                it = QTableWidgetItem(str(val))
                it.setTextAlignment(
                    Qt.AlignmentFlag.AlignCenter | Qt.AlignmentFlag.AlignVCenter)
                self.word_table.setItem(i, col, it)
            jpdb_val = self._jpdb_freq_map.get(lemma)
            freq_it = QTableWidgetItem(str(jpdb_val) if jpdb_val else "—")
            freq_it.setTextAlignment(
                Qt.AlignmentFlag.AlignCenter | Qt.AlignmentFlag.AlignVCenter)
            self.word_table.setItem(i, 6, freq_it)
            status_it = QTableWidgetItem("未同步")
            status_it.setTextAlignment(
                Qt.AlignmentFlag.AlignCenter | Qt.AlignmentFlag.AlignVCenter)
            self.word_table.setItem(i, 7, status_it)
        self._row_index = {
            self.word_table.item(i, 1).text(): i
            for i in range(self.word_table.rowCount())
            if self.word_table.item(i, 1)
        }
        self.word_table.blockSignals(False)
        self._apply_learning_status(
            auto_uncheck=self.chk_auto_uncheck_studied.isChecked())

    def _set_all_checked(self, checked: bool):
        state = Qt.CheckState.Checked if checked else Qt.CheckState.Unchecked
        self.word_table.blockSignals(True)
        for i in range(self.word_table.rowCount()):
            item = self.word_table.item(i, 0)
            if item:
                lemma_it = self.word_table.item(i, 1)
                st = self._learning_status.get(lemma_it.text()) if lemma_it else None
                if checked and self.chk_auto_uncheck_studied.isChecked() and st and st.studied:
                    item.setCheckState(Qt.CheckState.Unchecked)
                else:
                    item.setCheckState(state)
        self.word_table.blockSignals(False)
        self._update_sel_count()

    def _update_sel_count(self):
        n = sum(
            1 for i in range(self.word_table.rowCount())
            if self.word_table.item(i, 0) and
               self.word_table.item(i, 0).checkState() == Qt.CheckState.Checked
        )
        self.lbl_sel.setText(self._tr("selected", n=n))
        ok_conn = "✅" in self.anki_status.text()
        enabled = n > 0 and ok_conn
        self.btn_export.setEnabled(enabled)
        self.btn_update.setEnabled(enabled)
        self.btn_refresh_deck.setEnabled(ok_conn)

    # ---- Export ----

    def _start_export(self):
        selected = [
            {"lemma": self._word_rows[i][0], "pos": self._word_rows[i][1]}
            for i in range(self.word_table.rowCount())
            if self.word_table.item(i, 0) and
               self.word_table.item(i, 0).checkState() == Qt.CheckState.Checked
        ]
        if not selected:
            return

        song_ids = self.ex_song.selectedValues()
        artists  = self.ex_artist.selectedValues()
        if song_ids:
            ph = ",".join("?" * len(song_ids))
            song_clause, song_params = f"AND s.id IN ({ph})", list(song_ids)
        elif artists:
            c = " OR ".join(
                ["'/' || s.artist || '/' LIKE '%/' || ? || '/%'"] * len(artists)
            )
            song_clause, song_params = f"AND ({c})", list(artists)
        else:
            song_clause, song_params = "", []

        raw_lang = self.def_lang_combo.currentText()
        def_lang = "中文" if "中文" in raw_lang else "英文"
        cfg = {
            "deck":         self.deck_combo.currentText(),
            "fetch_def":    self.chk_fetch_def.isChecked(),
            "def_lang":     def_lang,
            "clip_audio":   self.chk_clip_audio.isEnabled() and
                            self.chk_clip_audio.isChecked(),
            "max_examples": self.max_ex_spin.value(),
            "dup_mode":     self.dup_mode_combo.currentText(),
            "dup_scope":    self.dup_scope_combo.currentText(),
            "song_clause":  song_clause,
            "song_params":  song_params,
            "jlpt_map":     dict(self._jlpt_map),
        }

        self._set_busy(True)
        self.log_list.clear()
        self.log_list.setVisible(True)
        self.progress_bar.setMaximum(len(selected))
        self.progress_bar.setValue(0)
        self.progress_bar.setVisible(True)
        self.lbl_prog.setText("開始…")

        self._worker = AnkiExportWorker(selected, cfg)
        self._worker.progress.connect(self._on_progress)
        self._worker.card_done.connect(self._on_card_done)
        self._worker.finished.connect(self._on_done)
        self._worker.review_needed.connect(self._on_review_needed)
        self._worker.start()

    def _start_update(self):
        selected = [
            {"lemma": self._word_rows[i][0], "pos": self._word_rows[i][1]}
            for i in range(self.word_table.rowCount())
            if self.word_table.item(i, 0) and
               self.word_table.item(i, 0).checkState() == Qt.CheckState.Checked
        ]
        if not selected:
            return

        raw_lang = self.def_lang_combo.currentText()
        def_lang = "中文" if "中文" in raw_lang else "英文"
        cfg = {
            "deck":      self.deck_combo.currentText(),
            "fetch_def": self.chk_fetch_def.isChecked(),
            "def_lang":  def_lang,
            "dup_scope": self.dup_scope_combo.currentText(),
            "jlpt_map":  dict(self._jlpt_map),
        }

        self._set_busy(True)
        self.progress_bar.setMaximum(len(selected))
        self.progress_bar.setValue(0)
        self.progress_bar.setVisible(True)
        self.lbl_prog.setText("更新中…")

        self._worker = AnkiUpdateWorker(selected, cfg)
        self._worker.progress.connect(self._on_progress)
        self._worker.finished.connect(self._on_done)
        self._worker.start()

    def _start_refresh_deck(self):
        deck = self.deck_combo.currentText().strip()
        if not deck:
            QMessageBox.warning(self, "エラー", "先选择要刷新的 Anki 牌组。")
            return
        refresh_scope = self.refresh_scope_combo.currentText()
        target = {
            "主牌组+子牌组": deck.split("::")[0],
            "当前牌组+子牌组": deck + " 及其子牌组",
        }.get(refresh_scope, deck)
        answer = QMessageBox.question(
            self,
            "刷新旧牌组",
            f"将扫描「{target}」中的 JPOP Corpus 卡片，并刷新：\n\n"
            "・新版中文释义 / 英文兜底\n"
            "・词性标注\n"
            "・JLPT / 声调 / JPDB 词频\n"
            "・卡片模板 CSS\n\n"
            "不会改动已有例句和音频。是否继续？",
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
            QMessageBox.StandardButton.No,
        )
        if answer != QMessageBox.StandardButton.Yes:
            return

        raw_lang = self.def_lang_combo.currentText()
        def_lang = "中文" if "中文" in raw_lang else "英文"
        cfg = {
            "deck": deck,
            "fetch_def": self.chk_fetch_def.isChecked(),
            "def_lang": def_lang,
            "refresh_scope": refresh_scope,
            "jlpt_map": dict(self._jlpt_map),
        }

        self._set_busy(True)
        self.log_list.clear()
        self.log_list.setVisible(True)
        self.progress_bar.setMaximum(1)
        self.progress_bar.setValue(0)
        self.progress_bar.setVisible(True)
        self.lbl_prog.setText("旧牌组扫描中…")

        self._worker = AnkiDeckRefreshWorker(cfg)
        self._worker.progress.connect(self._on_progress)
        self._worker.card_done.connect(self._on_card_done)
        self._worker.finished.connect(self._on_done)
        self._worker.start()

    def _set_busy(self, busy: bool):
        self.btn_export.setEnabled(not busy)
        self.btn_update.setEnabled(not busy)
        self.btn_refresh_deck.setEnabled(not busy)
        self.refresh_scope_combo.setEnabled(not busy)
        self.btn_query.setEnabled(not busy)
        self.btn_sync_learning.setEnabled(
            (not busy) and not (
                self._learning_worker and self._learning_worker.isRunning()))

    def _on_progress(self, done: int, total: int, word: str):
        if total and self.progress_bar.maximum() != total:
            self.progress_bar.setMaximum(total)
        self.progress_bar.setValue(done)
        self.lbl_prog.setText(f"[{done}/{total}]  処理中：{word}")

    def _on_card_done(self, lemma: str, status: str):
        from PyQt6.QtWidgets import QListWidgetItem
        from PyQt6.QtGui import QColor
        item = QListWidgetItem(f"{lemma}  {status}")
        if status.startswith("❌"):
            item.setForeground(QColor("#c0392b"))
        elif status.startswith("✅"):
            item.setForeground(QColor("#27ae60"))
        elif status.startswith("🔄"):
            item.setForeground(QColor("#2471a3"))
        self.log_list.addItem(item)
        self.log_list.scrollToBottom()
        # Advance the progress bar one step after each card completes
        self.progress_bar.setValue(self.progress_bar.value() + 1)

    def _on_review_needed(self, lemma: str, existing: str, candidates: list):
        dlg = SentenceReviewDialog(lemma, existing, candidates, self)
        if dlg.exec() == QDialog.DialogCode.Accepted:
            approved = dlg.approved_parts()
        else:
            approved = []
        self._worker.set_review_result(approved)

    def _on_done(self, ok: bool, msg: str):
        self._set_busy(False)
        self._update_sel_count()
        self.progress_bar.setVisible(False)
        # Split summary line from error detail
        summary, _, detail = msg.partition("\n\n失败明细：\n")
        self.lbl_prog.setText(summary)
        if not ok:
            QMessageBox.critical(self, "エラー", msg)
        elif detail:
            # Show errors in a scrollable dialog
            from PyQt6.QtWidgets import QDialog, QVBoxLayout, QTextEdit, QPushButton
            dlg = QDialog(self)
            dlg.setWindowTitle("失败明细")
            dlg.setMinimumSize(480, 300)
            lay = QVBoxLayout(dlg)
            te = QTextEdit()
            te.setReadOnly(True)
            te.setPlainText(summary + "\n\n" + detail)
            te.setStyleSheet("font-family:'Consolas','Courier New',monospace;font-size:12px")
            lay.addWidget(te)
            btn = QPushButton("閉じる")
            btn.clicked.connect(dlg.accept)
            lay.addWidget(btn)
            dlg.exec()
        else:
            QMessageBox.information(self, "完了", summary)
        if ok:
            QTimer.singleShot(300, self._start_learning_sync)

    def _stop_workers(self):
        if self._jlpt_worker and self._jlpt_worker.isRunning():
            try:
                self._jlpt_worker.result.disconnect()
                self._jlpt_worker.done.disconnect()
            except Exception:
                pass
            self._jlpt_worker.requestInterruption()
        if self._worker and self._worker.isRunning():
            try:
                self._worker.progress.disconnect()
                if hasattr(self._worker, "card_done"):
                    self._worker.card_done.disconnect()
                self._worker.finished.disconnect()
            except Exception:
                pass
            self._worker.requestInterruption()
        if self._learning_worker and self._learning_worker.isRunning():
            try:
                self._learning_worker.done.disconnect()
            except Exception:
                pass
            self._learning_worker.requestInterruption()

    def accept(self):
        self._stop_workers()
        super().accept()

    def reject(self):
        self._stop_workers()
        super().reject()

    def closeEvent(self, event):
        self._stop_workers()
        super().closeEvent(event)
