// 导出 Yomitan 日语文本预处理的码表和基准，给 Rust 移植对账用。
// 用法: node dump-yomitan-text-processing.mjs <yomitan 根目录> <基准输出目录> <crate data 目录> <crate tests/fixtures 目录> [额外输入.txt]
//
// 产物：
//   <data>/japanese-text-tables.json          Rust 运行时码表：半角片假名表、罗马字表原样取自 Yomitan 源码；
//                                              NFKD 映射和字符范围在 Node 里逐码位算出（和 JS 引擎的 Unicode 数据一致）
//   <fixtures>/japanese-text-pipeline.json    少量特殊输入的完整管线结果（入库，单测用）
//   <out>/japanese-text-pipeline-full.json    全部输入的完整管线结果（不入库，对账用）
//
// 管线 = Translator._findTermsInternal 的截断 + _getAlgorithmDeinflections（逐字缩短 → 预处理变体 → 活用还原 → 后处理变体）。
import {registerHooks} from 'node:module';
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {fileURLToPath, pathToFileURL} from 'node:url';
import {join} from 'node:path';

const [root, outDir, dataDir, fixturesDir, extraPath] = process.argv.slice(2);
for (const d of [outDir, dataDir, fixturesDir]) { mkdirSync(d, {recursive: true}); }

// ext/lib/* 是构建时从 npm 包生成的（kanji-processor、hangul-js 等），源码仓库里没有。
// 按导入方实际写的名字造恒等函数的桩。日语只用到 kanji-processor 的 convertVariants
// （standardizeKanji，旧字体→新字体）——Rust 侧暂未移植，两边一致地跳过这一步；其余桩只属于别的语言，不会被调用。
const stubbed = [];
registerHooks({
    resolve(specifier, context, nextResolve) {
        try {
            return nextResolve(specifier, context);
        } catch (e) {
            if (e?.code !== 'ERR_MODULE_NOT_FOUND' || !specifier.includes('/lib/') || !context.parentURL) { throw e; }
            const parentSource = readFileSync(fileURLToPath(context.parentURL), 'utf8');
            const escaped = specifier.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
            const names = new Set();
            let hasDefault = false;
            for (const m of parentSource.matchAll(new RegExp(`import\\s+([^;]*?)\\s+from\\s+['"]${escaped}['"]`, 'g'))) {
                const clause = m[1];
                const braces = clause.match(/\{([^}]*)\}/);
                if (braces) {
                    for (const part of braces[1].split(',')) {
                        const name = part.trim().split(/\s+as\s+/)[0].trim();
                        if (name) { names.add(name); }
                    }
                }
                const rest = clause.replace(/\{[^}]*\}/, '').replace(/,/g, ' ').trim();
                if (rest.length > 0 && !rest.startsWith('*')) { hasDefault = true; }
            }
            const src = [...names].map((n) => `export function ${n}(x) { return x; }`).join('\n') +
                (hasDefault ? '\nexport default function (x) { return x; }' : '');
            stubbed.push(`${specifier} {${[...names].join(', ')}}${hasDefault ? ' +default' : ''}`);
            return {url: 'data:text/javascript,' + encodeURIComponent(src), shortCircuit: true};
        }
    },
});

const imp = (p) => import(pathToFileURL(join(root, p)).href);
const {getAllLanguageTextProcessors} = await imp('ext/js/language/languages.js');
const {MAX_PROCESS_VARIANTS} = await imp('ext/js/language/text-processors.js');
const {japaneseTransforms} = await imp('ext/js/language/ja/japanese-transforms.js');
const {LanguageTransformer} = await imp('ext/js/language/language-transformer.js');
const {ROMAJI_TO_HIRAGANA} = await imp('ext/js/language/ja/japanese-kana-romaji-dicts.js');
const {isCodePointJapanese} = await imp('ext/js/language/ja/japanese.js');
const {isCodePointChinese} = await imp('ext/js/language/zh/chinese.js');
const {isCodePointKorean} = await imp('ext/js/language/ko/korean.js');
const {CJK_COMPATIBILITY, CJK_RADICALS_RANGES} = await imp('ext/js/language/CJK-util.js');

const ja = getAllLanguageTextProcessors().find((x) => x.iso === 'ja');
const preprocessors = ja.textPreprocessors;
const postprocessors = ja.textPostprocessors ?? [];

// ---- 码表 ----
const jaSrc = readFileSync(join(root, 'ext/js/language/ja/japanese.js'), 'utf8');
const mapLiteral = (name) => {
    const m = jaSrc.match(new RegExp(`const ${name} = new Map\\((\\[[\\s\\S]*?\\n\\])\\);`));
    if (!m) { throw new Error(`${name} not found`); }
    return new Function(`return ${m[1]};`)();
};
const halfwidth = mapLiteral('HALFWIDTH_KATAKANA_MAPPING');
for (const [k, v] of halfwidth) {
    if (k.length !== 1 || v.length !== 3) { throw new Error(`halfwidth mapping ${k} -> ${v}`); }
}
const nfkdTable = (ranges) => {
    const out = [];
    for (const [min, max] of ranges) {
        for (let cp = min; cp <= max; ++cp) {
            const s = String.fromCodePoint(cp);
            const n = s.normalize('NFKD');
            if (n !== s) { out.push([cp, n]); }
        }
    }
    return out;
};
const rangesOf = (predicate) => {
    const out = [];
    for (let cp = 0; cp <= 0x10ffff; ++cp) {
        if (!predicate(cp)) { continue; }
        const last = out[out.length - 1];
        if (last && last[1] === cp - 1) { last[1] = cp; } else { out.push([cp, cp]); }
    }
    return out;
};
const tables = {
    source: 'yomitan d34832d756e05dc00945e5b7d7ebc80963299a7a',
    preprocessorIds: preprocessors.map((p) => p.id),
    maxProcessVariants: MAX_PROCESS_VARIANTS,
    halfwidthKatakana: [...halfwidth],
    vowelToKana: [...mapLiteral('VOWEL_TO_KANA_MAPPING')],
    romajiToHiragana: Object.entries(ROMAJI_TO_HIRAGANA),
    cjkCompatibilityNfkd: nfkdTable([CJK_COMPATIBILITY]),
    radicalsNfkd: nfkdTable(CJK_RADICALS_RANGES),
    japaneseRanges: rangesOf(isCodePointJapanese),
    lookupRanges: rangesOf((cp) => isCodePointJapanese(cp) || isCodePointChinese(cp) || isCodePointKorean(cp)),
};
writeFileSync(join(dataDir, 'japanese-text-tables.json'), JSON.stringify(tables));

// ---- 管线（逐行照抄 translator.js）----
function getProcessedTexts(textCache, text, id, process) {
    let level1 = textCache.get(text);
    if (!level1) { level1 = new Map(); textCache.set(text, level1); }
    let results = level1.get(id);
    if (typeof results === 'undefined') {
        results = process(text);
        if (results.length > MAX_PROCESS_VARIANTS) { results = results.slice(0, MAX_PROCESS_VARIANTS); }
        level1.set(id, results);
    }
    return results;
}
function getTextVariants(text, textProcessors, textCache) {
    let variantsMap = new Map([[text, [[]]]]);
    for (const {id, textProcessor: {process}} of textProcessors) {
        const newVariantsMap = new Map();
        for (const [variant, currentPreprocessorRuleChainCandidates] of variantsMap) {
            for (const processed of getProcessedTexts(textCache, variant, id, process)) {
                const existingCandidates = newVariantsMap.get(processed);
                if (processed === variant) {
                    if (typeof existingCandidates === 'undefined') {
                        newVariantsMap.set(processed, currentPreprocessorRuleChainCandidates);
                    } else {
                        newVariantsMap.set(processed, existingCandidates);
                    }
                } else if (typeof existingCandidates === 'undefined') {
                    newVariantsMap.set(processed, currentPreprocessorRuleChainCandidates.map((candidate) => [...candidate, id]));
                } else {
                    newVariantsMap.set(processed, [...existingCandidates, ...currentPreprocessorRuleChainCandidates.map((candidate) => [...candidate, id])]);
                }
            }
        }
        variantsMap = newVariantsMap;
    }
    return variantsMap;
}
function cjkOnly(text) {
    let length = 0;
    for (const c of text) {
        const codePoint = c.codePointAt(0);
        if (!isCodePointJapanese(codePoint) && !isCodePointChinese(codePoint) && !isCodePointKorean(codePoint)) {
            return text.substring(0, length);
        }
        length += c.length;
    }
    return text;
}
const lt = new LanguageTransformer();
lt.addDescriptor(japaneseTransforms);
function pipeline(text) {
    const lines = [];
    const cache = new Map();
    for (let raw = text; raw.length > 0; raw = raw.substring(0, raw.length - 1)) {
        for (const [source, preCandidates] of getTextVariants(raw, preprocessors, cache)) {
            for (const {text: deinflected, conditions, trace} of lt.transform(source)) {
                for (const [transformed, postCandidates] of getTextVariants(deinflected, postprocessors, cache)) {
                    const candidates = preCandidates.flatMap((a) => postCandidates.map((b) => [...a, ...b]));
                    lines.push(`${raw}\t${source}\t${transformed}\t${conditions}\t${JSON.stringify(candidates)}\t${trace.map((f) => f.transform).join(',')}`);
                }
            }
        }
    }
    return lines;
}

const special = [
    'ﾖﾐﾁｬﾝ', 'ｶﾞｯｺｳ', 'ﾊﾟﾋﾟﾌﾟﾍﾟﾎﾟ', 'ｳﾞｧｲｵﾘﾝ', 'ﾅﾞ', 'ｰｰ', 'ｱﾞ',
    'yomichan', 'TABEMASU', 'ｙｏｍｉｔａｎ', 'utsu', 'kitte', 'tttsu', 'ttttttttttsu', 'nn', 'n', "shin'ya", 'kya-', 'ABC-ｱｲｳ－xyz',
    'ド', 'がき゚ぱ', '゙か', 'ゔ', 'パン',
    '㌀', '㍻', '⼀⼆', '⺟',
    'すっっごーーい', 'ーっすごいっっ', 'っっ', 'ーー', 'スッッゴーーイ', 'すっーーごい',
    'カタカナとひらがな', 'ゲーム', 'ラーメン', 'コーヒー', 'ヵヶ', 'ヴァ', 'のー', 'ノー',
    'Ｅｎｇｌｉｓｈ', 'USB', '39', '３９', 'テキスト', 'ウツ', 'ｳﾂ', 'てきすと',
    '打ち込んでいませんでした', 'のたもうた', '萬', '食べちゃった', '「君の名は。」', 'English words 混じり', '',
];
const testCases = JSON.parse(readFileSync(join(outDir, 'japanese-transforms-tests.json'), 'utf8'));
const testSources = testCases.flatMap((c) => c.tests.map((t) => t.source));
const extra = extraPath ? readFileSync(extraPath, 'utf8').split(/\r?\n/).filter((s) => s.length > 0) : [];

const hasAstral = (s) => [...s].some((c) => c.codePointAt(0) > 0xffff);
let skippedAstral = 0;
const dump = (inputs) => inputs.filter((s) => {
    if (hasAstral(s)) { ++skippedAstral; return false; }
    return true;
}).map((s) => [s, cjkOnly(s), pipeline(s)]);

const specialDump = dump(special);
writeFileSync(join(fixturesDir, 'japanese-text-pipeline.json'), JSON.stringify(specialDump, null, 1));
const full = dump([...new Set([...special, ...testSources, ...extra])]);
writeFileSync(join(outDir, 'japanese-text-pipeline-full.json'), JSON.stringify(full));

console.log(JSON.stringify({
    preprocessorIds: tables.preprocessorIds,
    postprocessors: postprocessors.map((p) => p.id),
    maxProcessVariants: MAX_PROCESS_VARIANTS,
    halfwidth: tables.halfwidthKatakana.length,
    romaji: tables.romajiToHiragana.length,
    cjkCompatibilityNfkd: tables.cjkCompatibilityNfkd.length,
    radicalsNfkd: tables.radicalsNfkd.length,
    japaneseRanges: tables.japaneseRanges.length,
    lookupRanges: tables.lookupRanges.length,
    specialInputs: specialDump.length,
    specialLines: specialDump.reduce((n, [, , l]) => n + l.length, 0),
    fullInputs: full.length,
    fullLines: full.reduce((n, [, , l]) => n + l.length, 0),
    skippedAstral,
    stubbed,
}, null, 1));
