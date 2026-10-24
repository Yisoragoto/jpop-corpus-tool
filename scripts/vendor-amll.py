"""把 @applemusic-like-lyrics/core 0.6.0 的发布包搬进 app/vendor/amll-core，并打上点词查词的补丁。"""
import json
import os
import shutil
import sys

src, dst = sys.argv[1], sys.argv[2]
os.makedirs(os.path.join(dst, 'dist'), exist_ok=True)
for name in ['amll-core.mjs', 'amll-core.d.mts', 'style.css', 'rolldown-runtime-D7D4PA-g.mjs']:
    shutil.copyfile(os.path.join(src, 'dist', name), os.path.join(dst, 'dist', name))
shutil.copyfile(os.path.join(src, 'LICENSE'), os.path.join(dst, 'LICENSE'))

def patch(path, old, new, count=1):
    text = open(path, encoding='utf-8').read()
    assert text.count(old) == count, (path, old, text.count(old))
    text = text.replace(old, new)
    open(path, 'w', encoding='utf-8', newline='').write(text)

mjs = os.path.join(dst, 'dist', 'amll-core.mjs')
# 1. 词元素记下它在原文里的字符位置（调用方在 LyricWord 上给 charStart）
patch(mjs,
      '\tcreateWord(word, shouldEmphasize) {\n\t\tconst mainWordEl = document.createElement("span");\n',
      '\tcreateWord(word, shouldEmphasize) {\n\t\tconst mainWordEl = document.createElement("span");\n'
      '\t\t// JPOP Corpus Tool patch: 词在原文里的字符位置，点词查词用\n'
      '\t\tif (word.charStart !== void 0) mainWordEl.dataset.charStart = String(word.charStart);\n')
# 2. 行点击事件带上被点中的词元素
patch(mjs,
      '\tisPropagationStopped = false;\n\tconstructor(lineIndex, line, bgLine, event) {',
      '\tisPropagationStopped = false;\n'
      '\t/** JPOP Corpus Tool patch: 被点中的词元素（带 data-char-start），没点在词上为 null */\n'
      '\twordElement = null;\n'
      '\tconstructor(lineIndex, line, bgLine, event) {')
patch(mjs,
      '\t\tconst evt = new LyricLineMouseEvent(this.lyricLinesIndexes.get(mainLine) ?? -1, mainLine, bgLine, e);\n',
      '\t\tconst evt = new LyricLineMouseEvent(this.lyricLinesIndexes.get(mainLine) ?? -1, mainLine, bgLine, e);\n'
      '\t\t// JPOP Corpus Tool patch\n'
      '\t\tevt.wordElement = target.closest("[data-char-start]");\n')

# 3. 普通词在拆空白 / 拆 CJK 单字时会重建成新对象，额外字段丢掉：按码点游标把 charStart 带过去
patch(mjs,
      '\t\tlet currentOffset = 0;\n\t\tlet nonSpaceIndex = 0;\n\t\tfor (const part of parts) {\n\t\t\tif (!part.trim()) {\n\t\t\t\tconst startTime = w.startTime + currentOffset * timePerUnit;\n\t\t\t\tprocessAtom({\n\t\t\t\t\tword: part,\n\t\t\t\t\tromanWord: "",\n\t\t\t\t\tstartTime,\n\t\t\t\t\tendTime: startTime,\n\t\t\t\t\tobscene\n\t\t\t\t});\n\t\t\t\tcontinue;\n\t\t\t}',
      '\t\tlet currentOffset = 0;\n\t\tlet nonSpaceIndex = 0;\n'
      '\t\t// JPOP Corpus Tool patch: 原文位置游标（Unicode 码点）\n'
      '\t\tlet charCursor = 0;\n'
      '\t\tconst charStartAt = () => w.charStart === void 0 ? void 0 : w.charStart + charCursor;\n'
      '\t\tfor (const part of parts) {\n\t\t\tif (!part.trim()) {\n\t\t\t\tconst startTime = w.startTime + currentOffset * timePerUnit;\n\t\t\t\tprocessAtom({\n\t\t\t\t\tword: part,\n\t\t\t\t\tromanWord: "",\n\t\t\t\t\tstartTime,\n\t\t\t\t\tendTime: startTime,\n\t\t\t\t\tobscene,\n\t\t\t\t\tcharStart: charStartAt()\n\t\t\t\t});\n'
      '\t\t\t\tcharCursor += Array.from(part).length;\n'
      '\t\t\t\tcontinue;\n\t\t\t}')
patch(mjs,
      '\t\t\t\t\tprocessAtom({\n\t\t\t\t\t\tword: char,\n\t\t\t\t\t\tromanWord: "",\n\t\t\t\t\t\tstartTime,\n\t\t\t\t\t\tendTime: startTime + timePerUnit,\n\t\t\t\t\t\tobscene\n\t\t\t\t\t});\n',
      '\t\t\t\t\tprocessAtom({\n\t\t\t\t\t\tword: char,\n\t\t\t\t\t\tromanWord: "",\n\t\t\t\t\t\tstartTime,\n\t\t\t\t\t\tendTime: startTime + timePerUnit,\n\t\t\t\t\t\tobscene,\n\t\t\t\t\t\tcharStart: charStartAt()\n\t\t\t\t\t});\n'
      '\t\t\t\t\tcharCursor += Array.from(char).length;\n')
patch(mjs,
      '\t\t\t\tprocessAtom({\n\t\t\t\t\tword: part,\n\t\t\t\t\tromanWord: partRomanWord,\n\t\t\t\t\tstartTime,\n\t\t\t\t\tendTime: startTime + duration,\n\t\t\t\t\tobscene\n\t\t\t\t});\n',
      '\t\t\t\tprocessAtom({\n\t\t\t\t\tword: part,\n\t\t\t\t\tromanWord: partRomanWord,\n\t\t\t\t\tstartTime,\n\t\t\t\t\tendTime: startTime + duration,\n\t\t\t\t\tobscene,\n\t\t\t\t\tcharStart: charStartAt()\n\t\t\t\t});\n'
      '\t\t\t\tcharCursor += Array.from(part).length;\n')

dts = os.path.join(dst, 'dist', 'amll-core.d.mts')
patch(dts,
      '  /** 单词的注音内容 */\n  ruby?: LyricWordBase[];\n}',
      '  /** 单词的注音内容 */\n  ruby?: LyricWordBase[];\n'
      '  /** JPOP Corpus Tool patch: 这个词在原文里的起始字符下标（Unicode 码点），会写到词元素的 data-char-start 上 */\n'
      '  charStart?: number;\n}')
patch(dts,
      '  isPropagationStopped: boolean;\n  constructor(lineIndex: number',
      '  isPropagationStopped: boolean;\n'
      '  /** JPOP Corpus Tool patch: 被点中的词元素（带 data-char-start），没点在词上为 null */\n'
      '  wordElement: Element | null;\n'
      '  constructor(lineIndex: number')

upstream = json.load(open(os.path.join(src, 'package.json'), encoding='utf-8'))
pkg = {
    'name': upstream['name'],
    'version': upstream['version'] + '-jpop.1',
    'description': upstream['description'] + '（JPOP Corpus Tool 打过补丁的副本，见 PATCHES.md）',
    'license': upstream['license'],
    'repository': upstream['repository'],
    'type': 'module',
    'module': './dist/amll-core.mjs',
    'types': './dist/amll-core.d.mts',
    'exports': {
        '.': {'types': './dist/amll-core.d.mts', 'default': './dist/amll-core.mjs'},
        './style.css': './dist/style.css',
    },
    'dependencies': {k: v for k, v in upstream['dependencies'].items() if k not in ('vitest', 'deep-freeze')},
    # pixi 7.x；bulge-pinch 属于 pixi-filters 系列，适配 pixi 7 的是 5.x
    'peerDependencies': {k: ('^5.1.1' if k == '@pixi/filter-bulge-pinch' else '^7.4.3') for k in upstream['peerDependencies']},
}
json.dump(pkg, open(os.path.join(dst, 'package.json'), 'w', encoding='utf-8', newline='\n'), ensure_ascii=False, indent=2)
print('ok', pkg['dependencies'])
