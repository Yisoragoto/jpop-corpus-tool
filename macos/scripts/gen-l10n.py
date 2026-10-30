#!/usr/bin/env python3
"""生成 macOS 客户端的英文表，并检查界面里的中文都有英文。

英文表 = Windows 客户端的翻译（app/src/locales/en*.ts、backend.ts，键是中文原文，占位符 {name}）
       + macOS 自己的（macos/Localization/en.json，键里的插值写成 {0} {1}），后者优先。
所有键的占位符统一换成按出现顺序的 {0} {1} …，运行时 L10n 按这个模板匹配。

  python3 scripts/gen-l10n.py --out "<.app>/Contents/Resources"   写 l10n-en.json 和 *.lproj/Localizable.strings
  python3 scripts/gen-l10n.py --check                              Swift 里有没有翻译的中文字面量就失败
"""
import argparse, glob, json, pathlib, re, sys

HERE = pathlib.Path(__file__).resolve().parent.parent          # macos/
REPO = HERE.parent
HAN = re.compile(r"[\u4e00-\u9fff]")
# 不进界面的：冒烟测试的日志、翻译器自己（里面那串简体字是判断用的）
SKIP = {"SmokeRun.swift", "L10n.swift"}


def unescape_swift(s):
    out, i = [], 0
    while i < len(s):
        if s[i] == "\\" and i + 1 < len(s):
            c = s[i + 1]
            out.append({"n": "\n", "t": "\t", '"': '"', "\\": "\\", "'": "'", "0": "\0"}.get(c, "\\" + c))
            i += 2
            continue
        out.append(s[i]); i += 1
    return "".join(out)


def swift_literals(src):
    """普通字符串字面量（插值里可以有括号、字符串）。跳过注释、原始串、多行串"""
    i, n, out = 0, len(src), []
    while i < n:
        c = src[i]
        if src.startswith("//", i):
            j = src.find("\n", i); i = n if j < 0 else j; continue
        if src.startswith("/*", i):
            j = src.find("*/", i); i = n if j < 0 else j + 2; continue
        if c == "#" and i + 1 < n and src[i + 1] == '"':
            j = src.find('"#', i + 2); i = n if j < 0 else j + 2; continue
        if src.startswith('"""', i):
            j = src.find('"""', i + 3); i = n if j < 0 else j + 3; continue
        if c == '"':
            j, buf, k = i + 1, [], 0
            while j < n and src[j] != '"' and src[j] != "\n":
                if src[j] == "\\" and j + 1 < n and src[j + 1] == "(":
                    depth, m = 1, j + 2
                    while m < n and depth:
                        if src[m] == "(":
                            depth += 1
                        elif src[m] == ")":
                            depth -= 1
                        elif src[m] == '"':
                            m += 1
                            while m < n and src[m] != '"':
                                if src[m] == "\\":
                                    m += 1
                                m += 1
                        m += 1
                    buf.append("{%d}" % k); k += 1
                    j = m
                    continue
                if src[j] == "\\":
                    buf.append(src[j:j + 2]); j += 2; continue
                buf.append(src[j]); j += 1
            out.append(unescape_swift("".join(buf)))
            i = j + 1
            continue
        i += 1
    return out


def windows_table():
    table = {}
    files = sorted(glob.glob(str(REPO / "app/src/locales/en*.ts"))) + [str(REPO / "app/src/locales/backend.ts")]
    entry = re.compile(r'^\s*"((?:[^"\\]|\\.)*)"\s*:\s*"((?:[^"\\]|\\.)*)",?\s*$', re.M)
    for f in files:
        for m in entry.finditer(pathlib.Path(f).read_text(encoding="utf-8")):
            zh, en = json.loads('"' + m.group(1) + '"'), json.loads('"' + m.group(2) + '"')
            names = []

            def zsub(mm):
                if mm.group(0) not in names:
                    names.append(mm.group(0))
                return "{%d}" % names.index(mm.group(0))
            key = re.sub(r"\{[^{}]*\}", zsub, zh)
            if "|" in re.sub(r"\{[^{}|]*\}", "", en):
                pass
            # {n|单数|复数} 这种 Windows 的复数写法：取复数那一种
            en = re.sub(r"\{([^{}|]*)\|[^{}|]*\|([^{}|]*)\}", r"\2", en)
            en = re.sub(r"\{[^{}]*\}", lambda mm: "{%d}" % names.index(mm.group(0)) if mm.group(0) in names else mm.group(0), en)
            table.setdefault(key, en)
    return table


def mac_table():
    path = HERE / "Localization/en.json"
    raw = json.loads(path.read_text(encoding="utf-8")) if path.exists() else {}
    return {unescape_swift(k): unescape_swift(v) for k, v in raw.items()}


def merged():
    table = windows_table()
    table.update(mac_table())
    return table


def not_ui():
    """代码里的数据（日文虚词表、正则、双语的首次启动提示）：是中文/日文但不是要翻的界面文字"""
    path = HERE / "Localization/not-ui.json"
    return set(json.loads(path.read_text(encoding="utf-8"))) if path.exists() else set()


def literals_in_sources():
    skip = not_ui()
    found = {}
    for f in sorted((HERE / "Sources").rglob("*.swift")):
        if f.name in SKIP:
            continue
        for lit in swift_literals(f.read_text(encoding="utf-8")):
            if HAN.search(lit) and lit not in skip:
                found.setdefault(lit, f.relative_to(HERE).as_posix())
    return found


def strings_escape(s):
    return s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")


def write(out):
    table = merged()
    exact = {k: v for k, v in table.items() if not re.search(r"\{\d+\}", k)}
    patterns = [[k, v] for k, v in table.items() if re.search(r"\{\d+\}", k) and HAN.search(re.sub(r"\{\d+\}", "", k))]
    out.mkdir(parents=True, exist_ok=True)
    (out / "l10n-en.json").write_text(json.dumps({"exact": exact, "patterns": patterns}, ensure_ascii=False), encoding="utf-8")
    # SwiftUI 用 LocalizedStringKey 的地方（alert 标题、Section、菜单名、输入框提示）走系统的表：只放没有插值的
    en = out / "en.lproj"; en.mkdir(exist_ok=True)
    (en / "Localizable.strings").write_text(
        "".join(f'"{strings_escape(k)}" = "{strings_escape(v)}";\n' for k, v in sorted(exact.items())), encoding="utf-8")
    zh = out / "zh-Hans.lproj"; zh.mkdir(exist_ok=True)
    (zh / "Localizable.strings").write_text("", encoding="utf-8")
    print(f"l10n: {len(exact)} exact + {len(patterns)} patterns -> {out}")


def check():
    table = merged()
    missing = [(lit, f) for lit, f in literals_in_sources().items() if lit not in table]
    for lit, f in missing:
        print(f"{f}: {json.dumps(lit, ensure_ascii=False)}")
    print(f"l10n check: {len(missing)} Chinese literals without English")
    return 1 if missing else 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--dump-missing", help="把缺的写成 JSON（键 → 用在哪个文件）")
    a = ap.parse_args()
    if a.dump_missing:
        table = merged()
        miss = {lit: f for lit, f in literals_in_sources().items() if lit not in table}
        pathlib.Path(a.dump_missing).write_text(json.dumps(miss, ensure_ascii=False, indent=1), encoding="utf-8")
        print(len(miss))
        return 0
    if a.out:
        write(pathlib.Path(a.out))
    if a.check:
        return check()
    return 0


sys.exit(main())
