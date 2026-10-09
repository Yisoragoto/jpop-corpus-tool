"""Yomitan dictionary manager dialog."""

from __future__ import annotations

from dialogs._bridge import install_gui_symbols

install_gui_symbols(globals())

from PyQt6.QtWidgets import QAbstractItemView

# ------------------------------------------------------------------ DictManagerDialog

class DictTableWidget(QTableWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.orderChanged = None

    def dropEvent(self, event):
        super().dropEvent(event)
        if callable(self.orderChanged):
            QTimer.singleShot(0, self.orderChanged)


class DictManagerDialog(QDialog):
    """Manage Yomitan dictionary imports — Term / Frequency / Pitch tabs with ordering."""

    _UI = {
        "zh": {
            "title": "词典管理",
            "subtitle": "管理 Yomitan 词条、词频和声调数据。词条词典支持拖拽排序，顺序会影响 Anki 释义优先级。",
            "tabs": [("词条", "terms"), ("词频", "freq"), ("声调", "pitch")],
            "headers": ["启用", "词典名", "版本", "词条数", "导入日期"],
            "up": "上移", "down": "下移", "add": "添加词典…", "delete": "删除所选",
            "close": "关闭", "error": "错误", "partial_fail": "部分失败",
            "note": "支持标准 Yomitan 格式 zip（index.json + term/freq/pitch bank）。词典顺序决定 Anki 卡片中释义的显示顺序。",
            "drag_hint": "拖拽词典行即可调整释义顺序",
            "fixed_hint": "此类数据按名称显示，不参与释义排序",
            "summary": "{count} 本词典 / {enabled} 本启用 / {entries} 条记录",
            "tip_up": "所选词典在释义中的优先级上移（仅词条词典）",
            "choose_zip": "选择 Yomitan 词典 zip", "zip_filter": "ZIP 文件 (*.zip)",
            "importing": "导入 {name}… {done}/{total}", "imported": "已导入：",
            "confirm_delete": "确认删除",
            "delete_msg": "删除词典「{name}」（{count} 词条）？{extra}",
            "warn_freq": "\n注意：将清除 yomitan_freq 表中的所有词频数据。",
            "warn_pitch": "\n注意：将清除 yomitan_pitch 表中的所有声调数据。",
            "deleted": "已删除：{name}",
        },
        "ja": {
            "title": "辞書管理",
            "subtitle": "Yomitan の語義・頻度・アクセントデータを管理します。語義辞書はドラッグで並べ替えられ、Anki の語義優先度に反映されます。",
            "tabs": [("語義", "terms"), ("頻度", "freq"), ("アクセント", "pitch")],
            "headers": ["有効", "辞書名", "版", "項目数", "取り込み日"],
            "up": "上へ", "down": "下へ", "add": "辞書を追加…", "delete": "選択を削除",
            "close": "閉じる", "error": "エラー", "partial_fail": "一部失敗",
            "note": "標準の Yomitan 形式 zip（index.json + term/freq/pitch bank）に対応。辞書の順序は Anki カード内の語義表示順に反映されます。",
            "drag_hint": "辞書行をドラッグして語義表示順を変更できます",
            "fixed_hint": "このデータ種別は名前順で表示され、語義順序には使われません",
            "summary": "{count} 辞書 / {enabled} 有効 / {entries} 項目",
            "tip_up": "選択した辞書の語義表示優先度を上げます（語義辞書のみ）",
            "choose_zip": "Yomitan 辞書 zip を選択", "zip_filter": "ZIP ファイル (*.zip)",
            "importing": "{name} を取り込み中… {done}/{total}", "imported": "取り込み済み：",
            "confirm_delete": "削除の確認",
            "delete_msg": "辞書「{name}」（{count} 項目）を削除しますか？{extra}",
            "warn_freq": "\n注意：yomitan_freq テーブルの頻度データをすべて削除します。",
            "warn_pitch": "\n注意：yomitan_pitch テーブルのアクセントデータをすべて削除します。",
            "deleted": "削除しました：{name}",
        },
    }

    def __init__(self, parent=None):
        super().__init__(parent)
        self.lang = load_settings().get("ui_language", "zh")
        cfg = load_settings()
        self.setFont(QFont(cfg.get("font_family", "Microsoft YaHei UI"), int(cfg.get("font_size", 11))))
        self.setWindowTitle(self._tr("title"))
        self.setMinimumWidth(980)
        self.setMinimumHeight(620)
        _ensure_dict_tables()
        _register_legacy_tables()
        self._worker = None
        self._current_type = "terms"
        self._build_ui()
        self._reload_table()

    def _tr(self, key: str, **kwargs):
        text = self._UI.get(self.lang, self._UI["zh"]).get(key, self._UI["zh"].get(key, key))
        return text.format(**kwargs) if kwargs and isinstance(text, str) else text

    # ── UI construction ──────────────────────────────────────────────────

    def _build_ui(self):
        self._apply_local_style()
        layout = QVBoxLayout(self)
        layout.setContentsMargins(24, 22, 24, 22)
        layout.setSpacing(14)

        header = QFrame()
        header.setObjectName("DictHeader")
        header_layout = QVBoxLayout(header)
        header_layout.setContentsMargins(18, 16, 18, 16)
        header_layout.setSpacing(8)
        title = QLabel(self._tr("title"))
        title.setObjectName("DictTitle")
        subtitle = QLabel(self._tr("subtitle"))
        subtitle.setObjectName("DictSubtitle")
        subtitle.setWordWrap(True)
        header_layout.addWidget(title)
        header_layout.addWidget(subtitle)
        layout.addWidget(header)

        toolbar = QFrame()
        toolbar.setObjectName("DictToolbar")
        toolbar_layout = QHBoxLayout(toolbar)
        toolbar_layout.setContentsMargins(14, 12, 14, 12)
        toolbar_layout.setSpacing(10)
        from PyQt6.QtWidgets import QButtonGroup
        self._tab_group = QButtonGroup(self)
        self._tab_group.setExclusive(True)
        for i, (label, dtype) in enumerate(self._tr("tabs")):
            btn = QPushButton(label)
            btn.setObjectName("DictSegment")
            btn.setCheckable(True)
            btn.setChecked(i == 0)
            btn.clicked.connect(lambda _, t=dtype: self._switch_tab(t))
            self._tab_group.addButton(btn, i)
            toolbar_layout.addWidget(btn)
        toolbar_layout.addSpacing(14)
        self.btn_add = QPushButton("＋ " + self._tr("add"))
        set_button_role(self.btn_add, "primary")
        self.btn_add.clicked.connect(self._add_dicts)
        toolbar_layout.addWidget(self.btn_add)
        self.btn_del = QPushButton(self._tr("delete"))
        set_button_role(self.btn_del, "danger")
        self.btn_del.clicked.connect(self._remove_dict)
        toolbar_layout.addWidget(self.btn_del)
        toolbar_layout.addStretch()
        self.lbl_summary = QLabel("")
        self.lbl_summary.setObjectName("DictSummary")
        toolbar_layout.addWidget(self.lbl_summary)
        layout.addWidget(toolbar)

        hint_row = QHBoxLayout()
        hint_row.setContentsMargins(4, 0, 4, 0)
        hint_row.setSpacing(10)
        self.lbl_hint = QLabel(self._tr("drag_hint"))
        self.lbl_hint.setObjectName("DictHint")
        hint_row.addWidget(self.lbl_hint)
        hint_row.addStretch()
        self.btn_up = QPushButton("↑ " + self._tr("up"))
        set_button_role(self.btn_up, "subtle")
        self.btn_up.setToolTip(self._tr("tip_up"))
        self.btn_up.clicked.connect(lambda: self._move_row(-1))
        hint_row.addWidget(self.btn_up)
        self.btn_dn = QPushButton("↓ " + self._tr("down"))
        set_button_role(self.btn_dn, "subtle")
        self.btn_dn.clicked.connect(lambda: self._move_row(1))
        hint_row.addWidget(self.btn_dn)
        layout.addLayout(hint_row)

        # Table
        self.table = DictTableWidget()
        self.table.setObjectName("DictTable")
        self.table.setColumnCount(5)
        self.table.setHorizontalHeaderLabels(self._tr("headers"))
        hh = self.table.horizontalHeader()
        hh.setSectionResizeMode(0, QHeaderView.ResizeMode.Fixed)
        hh.setSectionResizeMode(1, QHeaderView.ResizeMode.Stretch)
        hh.setSectionResizeMode(2, QHeaderView.ResizeMode.ResizeToContents)
        hh.setSectionResizeMode(3, QHeaderView.ResizeMode.ResizeToContents)
        hh.setSectionResizeMode(4, QHeaderView.ResizeMode.ResizeToContents)
        self.table.setColumnWidth(0, 46)
        self.table.setSelectionBehavior(QTableWidget.SelectionBehavior.SelectRows)
        self.table.setEditTriggers(QTableWidget.EditTrigger.NoEditTriggers)
        self.table.setDragEnabled(True)
        self.table.setAcceptDrops(True)
        self.table.setDropIndicatorShown(True)
        self.table.setDragDropOverwriteMode(False)
        self.table.setDefaultDropAction(Qt.DropAction.MoveAction)
        self.table.setDragDropMode(QAbstractItemView.DragDropMode.InternalMove)
        self.table.orderChanged = self._save_visible_order
        tune_table(self.table, 34)
        layout.addWidget(self.table)

        self.prog = QProgressBar()
        self.prog.setObjectName("DictProgress")
        self.prog.setVisible(False)
        self.prog.setTextVisible(True)
        layout.addWidget(self.prog)

        footer = QFrame()
        footer.setObjectName("DictFooter")
        footer_layout = QHBoxLayout(footer)
        footer_layout.setContentsMargins(14, 12, 14, 12)
        footer_layout.setSpacing(10)
        note = QLabel(self._tr("note"))
        note.setObjectName("DictNote")
        note.setWordWrap(True)
        footer_layout.addWidget(note, 1)
        self.lbl_status = QLabel("")
        self.lbl_status.setObjectName("DictStatus")
        footer_layout.addWidget(self.lbl_status)
        btn_close = QPushButton(self._tr("close"))
        set_button_role(btn_close, "subtle")
        btn_close.clicked.connect(self.accept)
        footer_layout.addWidget(btn_close)
        layout.addWidget(footer)

    def _apply_local_style(self, theme: str | None = None):
        theme = theme or load_settings().get("theme", "dark")
        if theme == "light":
            bg, panel, panel2, row_alt, fg, muted, border, accent, danger = (
                "#f4f7fb", "#ffffff", "#f8fafc", "#eef3f8", "#172033", "#64748b",
                "#d8e0ea", "#1f6fd1", "#b4232f"
            )
        else:
            bg, panel, panel2, row_alt, fg, muted, border, accent, danger = (
                "#1b1f25", "#232933", "#1f2329", "#252b33", "#edf4ff", "#9aa6b5",
                "#343c48", "#2f80ed", "#d85d67"
            )
        self.setStyleSheet(f"""
            QDialog {{
                background: {bg};
                color: {fg};
            }}
            QFrame#DictHeader, QFrame#DictToolbar, QFrame#DictFooter {{
                background: {panel};
                border: 1px solid {border};
                border-radius: 12px;
            }}
            QLabel#DictTitle {{
                color: {fg};
                font-size: 22px;
                font-weight: 760;
            }}
            QLabel#DictSubtitle, QLabel#DictHint, QLabel#DictNote, QLabel#DictStatus, QLabel#DictSummary {{
                color: {muted};
                font-size: 13px;
            }}
            QPushButton {{
                min-height: 34px;
                border: 1px solid {border};
                border-radius: 17px;
                padding: 4px 15px;
                background: {panel2};
                color: {fg};
            }}
            QPushButton:hover {{
                border-color: {accent};
                background: {row_alt};
            }}
            QPushButton:disabled {{
                color: {muted};
                background: transparent;
            }}
            QPushButton#DictSegment {{
                min-width: 84px;
                border-radius: 17px;
                font-weight: 650;
            }}
            QPushButton#DictSegment:checked {{
                background: {accent};
                border-color: {accent};
                color: white;
            }}
            QPushButton[role="primary"] {{
                background: {accent};
                border-color: {accent};
                color: white;
                font-weight: 700;
            }}
            QPushButton[role="danger"] {{
                border-color: {danger};
                color: {danger};
            }}
            QTableWidget#DictTable {{
                background: {panel2};
                alternate-background-color: {row_alt};
                color: {fg};
                gridline-color: transparent;
                border: 1px solid {border};
                border-radius: 12px;
                outline: none;
                selection-background-color: rgba(47, 128, 237, 0.28);
                selection-color: {fg};
                font-size: 14px;
            }}
            QTableWidget#DictTable::item {{
                padding: 7px 10px;
                border-bottom: 1px solid rgba(128, 144, 160, 0.08);
            }}
            QTableWidget#DictTable::item:hover {{
                background: {row_alt};
            }}
            QHeaderView::section {{
                background: {panel};
                color: {fg};
                border: none;
                border-right: 1px solid {border};
                border-bottom: 1px solid {border};
                padding: 8px 10px;
                font-weight: 700;
            }}
            QProgressBar#DictProgress {{
                min-height: 8px;
                max-height: 8px;
                border: none;
                border-radius: 4px;
                background: {border};
                text-align: center;
            }}
            QProgressBar#DictProgress::chunk {{
                border-radius: 4px;
                background: {accent};
            }}
        """)

    def refresh_theme(self, theme: str):
        self._apply_local_style(theme)

    # ── Tab / table ───────────────────────────────────────────────────────

    def _switch_tab(self, dtype: str):
        self._current_type = dtype
        self.btn_up.setEnabled(dtype == "terms")
        self.btn_dn.setEnabled(dtype == "terms")
        self.lbl_hint.setText(self._tr("drag_hint") if dtype == "terms" else self._tr("fixed_hint"))
        self.table.setDragEnabled(dtype == "terms")
        self.table.setAcceptDrops(dtype == "terms")
        self.table.setDragDropMode(
            QAbstractItemView.DragDropMode.InternalMove
            if dtype == "terms"
            else QAbstractItemView.DragDropMode.NoDragDrop
        )
        try:
            self.table.itemChanged.disconnect(self._on_toggle)
        except Exception:
            pass
        self._reload_table()

    def _reload_table(self):
        self.table.blockSignals(True)
        self.table.setRowCount(0)
        order_col = "sort_order ASC" if self._current_type == "terms" else "name ASC"
        try:
            conn = sqlite3.connect(str(DB_PATH))
            rows = conn.execute(
                f"SELECT name, revision, entry_count, enabled, imported_at "
                f"FROM dict_registry WHERE dict_type=? ORDER BY {order_col}",
                (self._current_type,)
            ).fetchall()
            conn.close()
        except Exception:
            rows = []

        self.table.setRowCount(len(rows))
        center = Qt.AlignmentFlag.AlignCenter | Qt.AlignmentFlag.AlignVCenter
        for i, (name, revision, count, enabled, date) in enumerate(rows):
            chk = QTableWidgetItem()
            chk.setFlags(Qt.ItemFlag.ItemIsEnabled | Qt.ItemFlag.ItemIsUserCheckable)
            chk.setCheckState(Qt.CheckState.Checked if enabled else Qt.CheckState.Unchecked)
            chk.setTextAlignment(center)
            self.table.setItem(i, 0, chk)
            self.table.setItem(i, 1, QTableWidgetItem(name))
            rev_it = QTableWidgetItem(revision or "—")
            rev_it.setToolTip(revision or "")
            self.table.setItem(i, 2, rev_it)
            cnt = QTableWidgetItem(f"{count:,}")
            cnt.setTextAlignment(center)
            self.table.setItem(i, 3, cnt)
            self.table.setItem(i, 4, QTableWidgetItem(date or ""))
        self.table.blockSignals(False)
        self.table.itemChanged.connect(self._on_toggle)

        is_terms = self._current_type == "terms"
        self.btn_up.setEnabled(is_terms)
        self.btn_dn.setEnabled(is_terms)
        self.table.setDragEnabled(is_terms)
        self.table.setAcceptDrops(is_terms)
        self.table.setDragDropMode(
            QAbstractItemView.DragDropMode.InternalMove
            if is_terms
            else QAbstractItemView.DragDropMode.NoDragDrop
        )
        self.lbl_hint.setText(self._tr("drag_hint") if is_terms else self._tr("fixed_hint"))
        total_entries = sum(int(row[2] or 0) for row in rows)
        enabled_count = sum(1 for row in rows if row[3])
        self.lbl_summary.setText(self._tr(
            "summary",
            count=len(rows),
            enabled=enabled_count,
            entries=f"{total_entries:,}",
        ))

    def _on_toggle(self, item: QTableWidgetItem):
        if item.column() != 0:
            return
        name_it = self.table.item(item.row(), 1)
        if not name_it:
            return
        enabled = 1 if item.checkState() == Qt.CheckState.Checked else 0
        try:
            conn = sqlite3.connect(str(DB_PATH))
            conn.execute(
                "UPDATE dict_registry SET enabled=? WHERE name=? AND dict_type=?",
                (enabled, name_it.text(), self._current_type),
            )
            conn.commit()
            conn.close()
        except Exception as e:
            QMessageBox.warning(self, self._tr("error"), str(e))

    # ── Reorder (Term only) ───────────────────────────────────────────────

    def _visible_names(self) -> list[str]:
        names = []
        for row in range(self.table.rowCount()):
            item = self.table.item(row, 1)
            if item and item.text():
                names.append(item.text())
        return names

    def _save_visible_order(self):
        if self._current_type != "terms":
            return
        names = self._visible_names()
        if not names:
            return
        try:
            conn = sqlite3.connect(str(DB_PATH))
            for order, name in enumerate(names):
                conn.execute(
                    "UPDATE dict_registry SET sort_order=? WHERE name=? AND dict_type='terms'",
                    (order, name),
                )
            conn.commit()
            conn.close()
            self.lbl_status.setText(self._tr("drag_hint"))
        except Exception as e:
            QMessageBox.warning(self, self._tr("error"), str(e))
            return
        current = self.table.currentRow()
        try:
            self.table.itemChanged.disconnect(self._on_toggle)
        except Exception:
            pass
        self._reload_table()
        if current >= 0:
            self.table.selectRow(min(current, self.table.rowCount() - 1))

    def _move_row(self, direction: int):
        """Swap sort_order of current row with the one above (-1) or below (+1)."""
        if self._current_type != "terms":
            return
        row = self.table.currentRow()
        target = row + direction
        if row < 0 or not (0 <= target < self.table.rowCount()):
            return
        name_a = self.table.item(row, 1).text()
        name_b = self.table.item(target, 1).text()
        try:
            conn = sqlite3.connect(str(DB_PATH))
            ord_a = conn.execute(
                "SELECT sort_order FROM dict_registry WHERE name=? AND dict_type='terms'", (name_a,)).fetchone()
            ord_b = conn.execute(
                "SELECT sort_order FROM dict_registry WHERE name=? AND dict_type='terms'", (name_b,)).fetchone()
            if ord_a and ord_b:
                conn.execute("UPDATE dict_registry SET sort_order=? WHERE name=? AND dict_type='terms'",
                             (ord_b[0], name_a))
                conn.execute("UPDATE dict_registry SET sort_order=? WHERE name=? AND dict_type='terms'",
                             (ord_a[0], name_b))
                conn.commit()
            conn.close()
        except Exception as e:
            QMessageBox.warning(self, self._tr("error"), str(e))
            return
        try:
            self.table.itemChanged.disconnect(self._on_toggle)
        except Exception:
            pass
        self._reload_table()
        self.table.selectRow(target)

    # ── Import ────────────────────────────────────────────────────────────

    def _add_dicts(self):
        paths, _ = QFileDialog.getOpenFileNames(
            self, self._tr("choose_zip"), "", self._tr("zip_filter"))
        if not paths:
            return
        self._set_busy(True)

        class _W(QThread):
            prog_sig = pyqtSignal(str, int, int)
            done_sig = pyqtSignal(list, list)
            def __init__(self, paths):
                super().__init__()
                self.paths = paths
            def run(self):
                successes, errors = [], []
                conn = sqlite3.connect(str(DB_PATH))
                try:
                    for path in self.paths:
                        fname = pathlib.Path(path).name
                        def _cb(done, total, s=fname):
                            self.prog_sig.emit(s, done, total)
                        try:
                            n, c, t = _import_single_yomitan_dict(path, conn, _cb)
                            successes.append((n, c, t))
                        except Exception as e:
                            errors.append(f"{fname}：{e}")
                finally:
                    conn.close()
                self.done_sig.emit(successes, errors)

        self._worker = _W(paths)
        self._worker.prog_sig.connect(self._on_prog)
        self._worker.done_sig.connect(self._on_import_done)
        self._worker.start()

    def _on_prog(self, name: str, done: int, total: int):
        self.prog.setMaximum(max(total, 1))
        self.prog.setValue(done)
        self.lbl_status.setText(self._tr("importing", name=name, done=done, total=total))

    def _on_import_done(self, successes: list, errors: list):
        self._set_busy(False)
        if errors:
            QMessageBox.warning(self, self._tr("partial_fail"), "\n".join(errors))
        if successes:
            self.lbl_status.setText(
                self._tr("imported") + "、".join(f"{n}（{c:,}）" for n, c, _ in successes))
            # Switch to the tab of the last imported dict
            last_type = successes[-1][2]
            for i, (_, dtype) in enumerate(self._tr("tabs")):
                if dtype == last_type:
                    self._tab_group.button(i).setChecked(True)
                    self._switch_tab(last_type)
                    return
        try:
            self.table.itemChanged.disconnect(self._on_toggle)
        except Exception:
            pass
        self._reload_table()

    # ── Delete ────────────────────────────────────────────────────────────

    def _remove_dict(self):
        row = self.table.currentRow()
        if row < 0:
            return
        name_it = self.table.item(row, 1)
        cnt_it  = self.table.item(row, 3)
        if not name_it:
            return
        name = name_it.text()
        cnt  = cnt_it.text() if cnt_it else "?"
        dtype = self._current_type
        extra = ""
        if dtype == "freq":
            extra = self._tr("warn_freq")
        elif dtype == "pitch":
            extra = self._tr("warn_pitch")
        if QMessageBox.question(
            self, self._tr("confirm_delete"),
            self._tr("delete_msg", name=name, count=cnt, extra=extra),
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No
        ) != QMessageBox.StandardButton.Yes:
            return
        try:
            conn = sqlite3.connect(str(DB_PATH))
            zip_path_row = conn.execute(
                "SELECT zip_path FROM dict_registry WHERE name=?", (name,)).fetchone()
            zip_path = zip_path_row[0] if zip_path_row else ""
            # Legacy tables use special markers; regular dicts use dict_terms
            if zip_path == _LEGACY_MEIKYO_KEY:
                _delete_legacy_terms_source(conn, "明鏡")
            elif zip_path == _LEGACY_SHOGAKUKAN_KEY:
                _delete_legacy_terms_source(conn, "小学館")
            elif zip_path == _LEGACY_PITCH_KEY:
                conn.execute("DELETE FROM yomitan_pitch")
            elif zip_path == _LEGACY_FREQ_KEY:
                conn.execute("DELETE FROM yomitan_freq")
            elif dtype == "terms":
                conn.execute("DELETE FROM dict_terms WHERE dict_name=?", (name,))
            elif dtype == "freq":
                conn.execute("DELETE FROM yomitan_freq")
            elif dtype == "pitch":
                conn.execute("DELETE FROM yomitan_pitch")
            conn.execute("DELETE FROM dict_registry WHERE name=?", (name,))
            conn.commit()
            conn.close()
        except Exception as e:
            QMessageBox.warning(self, self._tr("error"), str(e))
            return
        try:
            self.table.itemChanged.disconnect(self._on_toggle)
        except Exception:
            pass
        self._reload_table()
        self.lbl_status.setText(self._tr("deleted", name=name))

    # ── Helpers ───────────────────────────────────────────────────────────

    def _set_busy(self, busy: bool):
        self.btn_add.setEnabled(not busy)
        self.btn_del.setEnabled(not busy)
        can_sort = (not busy) and self._current_type == "terms"
        self.btn_up.setEnabled(can_sort)
        self.btn_dn.setEnabled(can_sort)
        self.table.setDragEnabled(can_sort)
        self.table.setAcceptDrops(can_sort)
        self.prog.setVisible(busy)
        if not busy:
            self.prog.setValue(0)
