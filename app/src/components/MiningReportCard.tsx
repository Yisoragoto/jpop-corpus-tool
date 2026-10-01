/**
 * 挖词报告。对应 PyQt 版菜单「报告 → 生成挖词报告…」。
 *
 * 读 Anki 的 collection（复制一份再读，不需要 Anki 开着），按歌手、歌曲、JLPT 统计已挖的词和复习进度，
 * 写成 `output/corpus_report.html` 用浏览器打开。页面和 Python 版 `generate_report.py` 写出的一样，
 * 只是多算了一键制卡做的 Lyrics 卡。
 */

import { useState } from "react";

import { api, type MiningReportResult } from "../api";
import { CommandButton } from "./CommandButton";

/** 「生成时间」：本地时间 `YYYY-MM-DD HH:MM`，和 Python 的 `strftime("%Y-%m-%d %H:%M")` 一样 */
export function localTimestamp(date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

interface Props {
  onError: (message: string) => void;
}

export function MiningReportCard({ onError }: Props) {
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<MiningReportResult | null>(null);

  const generate = async () => {
    setRunning(true);
    try {
      setResult(await api.ankiMiningReport(localTimestamp()));
    } catch (err) {
      setResult(null);
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="card">
      <div className="card-head">
        <h2>挖词报告</h2>
        {result && <span className="muted small">{result.noteTypes.join("、")}</span>}
      </div>
      <p className="muted small">
        按歌手、歌曲和 JLPT 统计已经挖进 Anki 的词（JPOP Corpus 和 Lyrics 两种卡）和复习进度，
        生成一个能搜索、能排序的网页，用浏览器打开。只读 Anki 的库，不需要 Anki 开着。
      </p>
      <div className="toolbar">
        <CommandButton
          icon="report"
          label={running ? "生成中…" : "生成挖词报告"}
          onClick={() => void generate()}
          disabled={running}
          primary
        />
      </div>
      {result && (
        <p className="small">
          {result.cards} 张卡（{result.studied} 张复习过）· {result.words} 个词 · {result.artists} 位歌手 ·{" "}
          {result.songs} 首歌
          <br />
          <span className="muted">
            {result.openError ? "没能自动打开浏览器，文件在：" : "已在浏览器打开："}
            <code>{result.path}</code>
          </span>
          {result.openError && <span className="warn"> {result.openError}</span>}
        </p>
      )}
    </div>
  );
}
