"""Token correction dialog."""

from __future__ import annotations

from dialogs._bridge import install_gui_symbols

install_gui_symbols(globals())

_POS_OPTIONS = [
    "NOUN", "VERB", "ADJ", "ADV", "ADP", "AUX", "CCONJ", "SCONJ",
    "DET", "INTJ", "NUM", "PART", "PRON", "PROPN", "PUNCT", "SYM", "X",
]


class TokenCorrectionDialog(QDialog):
    """Edit tokenization of one utterance: merge / split / retag / direct edit."""

    def __init__(self, utterance_id: int, text: str, artist: str = "",
                 title: str = "", parent=None):
        super().__init__(parent)
        self.utterance_id = utterance_id
        self._text   = text
        self._artist = artist
        self._title  = title
        self._orig_tokens: list = []
        self.setWindowTitle("分词校正")
        self.setMinimumWidth(640)
        self.setMinimumHeight(460)
        _ensure_token_corrections_table()
        self._build_ui(text)
        self._load_tokens()

    def _build_ui(self, text: str):
        lay = QVBoxLayout(self)
        lay.setSpacing(6)

        lbl = QLabel(text)
        lbl.setWordWrap(True)
        lbl.setStyleSheet("font-size:14px;font-weight:bold;padding:4px 0")
        lay.addWidget(lbl)

        info = QLabel("直接编辑单元格 · 多选相邻行 → 合并 · 单选行 → 拆分")
        info.setStyleSheet("font-size:11px;color:#888")
        lay.addWidget(info)

        self.tbl = QTableWidget()
        self.tbl.setColumnCount(3)
        self.tbl.setHorizontalHeaderLabels(["表層形", "語元 (lemma)", "品詞 (POS)"])
        hh = self.tbl.horizontalHeader()
        hh.setSectionResizeMode(0, QHeaderView.ResizeMode.Stretch)
        hh.setSectionResizeMode(1, QHeaderView.ResizeMode.Stretch)
        hh.setSectionResizeMode(2, QHeaderView.ResizeMode.ResizeToContents)
        self.tbl.setSelectionBehavior(QTableWidget.SelectionBehavior.SelectRows)
        tune_table(self.tbl, 32)
        self.tbl.itemSelectionChanged.connect(self._update_btn_state)
        lay.addWidget(self.tbl)

        btn_row = QHBoxLayout()
        self.btn_merge = QPushButton("合并选中行")
        set_button_role(self.btn_merge, "subtle")
        self.btn_merge.setToolTip("将多个相邻 token 合并为一个（保留首行的 lemma/POS）")
        self.btn_merge.setEnabled(False)
        self.btn_merge.clicked.connect(self._merge)
        btn_row.addWidget(self.btn_merge)

        self.btn_split = QPushButton("拆分…")
        set_button_role(self.btn_split, "subtle")
        self.btn_split.setToolTip("在指定字符位置将 token 一分为二")
        self.btn_split.setEnabled(False)
        self.btn_split.clicked.connect(self._split)
        btn_row.addWidget(self.btn_split)

        btn_row.addSpacing(16)
        self.btn_reset = QPushButton("重置为 GiNZA 原始")
        set_button_role(self.btn_reset, "danger")
        self.btn_reset.setToolTip("恢复 GiNZA 自动分词结果（撤销所有手动修改）")
        self.btn_reset.clicked.connect(self._reset)
        btn_row.addWidget(self.btn_reset)
        btn_row.addStretch()
        lay.addLayout(btn_row)

        note = QLabel("保存后立即更新 tokens 表，下次搜索时生效。重新导入该曲会覆盖校正。")
        note.setStyleSheet("font-size:11px;color:#888")
        note.setWordWrap(True)
        lay.addWidget(note)

        bb = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Save | QDialogButtonBox.StandardButton.Cancel)
        bb.accepted.connect(self._save)
        bb.rejected.connect(self.reject)
        lay.addWidget(bb)

    def _load_tokens(self):
        conn = sqlite3.connect(str(DB_PATH))
        ginza_raw = conn.execute(
            "SELECT surface, lemma, pos FROM tokens WHERE utterance_id=? ORDER BY token_idx",
            (self.utterance_id,)).fetchall()
        corr = conn.execute(
            "SELECT tokens_json, orig_json FROM token_corrections WHERE utterance_id=?",
            (self.utterance_id,)).fetchone()
        conn.close()

        ginza_tokens = [{"surface": s, "lemma": l, "pos": p} for s, l, p in ginza_raw]

        if corr:
            display_tokens = json.loads(corr[0])
            orig_raw = json.loads(corr[1]) if corr[1] and corr[1] != '[]' else ginza_tokens
            self._orig_tokens = orig_raw
        else:
            display_tokens = ginza_tokens
            self._orig_tokens = ginza_tokens

        self._fill_table(display_tokens)

    def _fill_table(self, tokens: list):
        self.tbl.blockSignals(True)
        self.tbl.setRowCount(len(tokens))
        center = Qt.AlignmentFlag.AlignCenter | Qt.AlignmentFlag.AlignVCenter
        for i, t in enumerate(tokens):
            self.tbl.setItem(i, 0, QTableWidgetItem(t.get("surface", "")))
            self.tbl.setItem(i, 1, QTableWidgetItem(t.get("lemma", t.get("surface", ""))))
            pos_it = QTableWidgetItem(t.get("pos", "NOUN"))
            pos_it.setTextAlignment(center)
            self.tbl.setItem(i, 2, pos_it)
        self.tbl.blockSignals(False)
        self._update_btn_state()

    def _current_tokens(self) -> list:
        tokens = []
        for i in range(self.tbl.rowCount()):
            s = (self.tbl.item(i, 0) or QTableWidgetItem("")).text().strip()
            l = (self.tbl.item(i, 1) or QTableWidgetItem("")).text().strip()
            p = (self.tbl.item(i, 2) or QTableWidgetItem("")).text().strip()
            if s:
                tokens.append({"surface": s, "lemma": l or s, "pos": p or "NOUN"})
        return tokens

    def _update_btn_state(self):
        sel = sorted({idx.row() for idx in self.tbl.selectedIndexes()})
        n = len(sel)
        consec = n >= 2 and (sel[-1] - sel[0] + 1 == n)
        self.btn_merge.setEnabled(consec)
        self.btn_split.setEnabled(n == 1)

    def _merge(self):
        sel = sorted({idx.row() for idx in self.tbl.selectedIndexes()})
        if len(sel) < 2:
            return
        tokens = self._current_tokens()
        merged = {
            "surface": "".join(tokens[i]["surface"] for i in sel),
            "lemma":   tokens[sel[0]]["lemma"],
            "pos":     tokens[sel[0]]["pos"],
        }
        new_tokens = tokens[:sel[0]] + [merged] + tokens[sel[-1] + 1:]
        self._fill_table(new_tokens)

    def _split(self):
        sel = sorted({idx.row() for idx in self.tbl.selectedIndexes()})
        if len(sel) != 1:
            return
        tokens = self._current_tokens()
        tok = tokens[sel[0]]
        surface = tok["surface"]
        if len(surface) < 2:
            QMessageBox.information(self, "拆分", "单字 token 无法继续拆分。")
            return

        dlg = QDialog(self)
        dlg.setWindowTitle("拆分位置")
        dlg.setFixedWidth(320)
        v = QVBoxLayout(dlg)
        v.addWidget(QLabel(f"表層形：「{surface}」\n第一部分保留几个字符？（1 ~ {len(surface)-1}）"))
        spin = QSpinBox()
        spin.setRange(1, len(surface) - 1)
        spin.setValue(max(1, len(surface) // 2))
        v.addWidget(spin)
        bb2 = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel)
        bb2.accepted.connect(dlg.accept)
        bb2.rejected.connect(dlg.reject)
        v.addWidget(bb2)
        if dlg.exec() != QDialog.DialogCode.Accepted:
            return

        pos = spin.value()
        p1, p2 = surface[:pos], surface[pos:]
        new_tokens = (tokens[:sel[0]]
                      + [{"surface": p1, "lemma": p1, "pos": tok["pos"]},
                         {"surface": p2, "lemma": p2, "pos": tok["pos"]}]
                      + tokens[sel[0] + 1:])
        self._fill_table(new_tokens)

    def _reset(self):
        if QMessageBox.question(
            self, "重置确认", "重置为 GiNZA 原始分词？手动修改将丢失。",
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No
        ) != QMessageBox.StandardButton.Yes:
            return
        self._fill_table(self._orig_tokens)

    def _save(self):
        tokens = self._current_tokens()
        if not tokens:
            QMessageBox.warning(self, "错误", "至少需要一个 token。")
            return
        try:
            _apply_token_correction(self.utterance_id, tokens, self._orig_tokens,
                                    text=self._text, artist=self._artist,
                                    title=self._title)
            self.accept()
        except Exception as e:
            QMessageBox.critical(self, "保存失败", str(e))
