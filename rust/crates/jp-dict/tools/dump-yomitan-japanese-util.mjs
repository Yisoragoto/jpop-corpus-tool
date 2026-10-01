// 注音分配、音高相关函数的基准：Yomitan 自带用例 + 真实词典词头，用 Yomitan 源码算出结果，给前端 TS 移植对账。
// 用法: node dump-yomitan-japanese-util.mjs <yomitan 根目录> <额外 词形\t读音 列表> <输出 json>
import {readFileSync, writeFileSync} from 'node:fs';
import {pathToFileURL} from 'node:url';
import {join} from 'node:path';

const [root, pairsPath, outPath] = process.argv.slice(2);
const jp = await import(pathToFileURL(join(root, 'ext/js/language/ja/japanese.js')).href);
const src = readFileSync(join(root, 'test/japanese-util.test.js'), 'utf8');

function dataOf(name) {
    const start = src.indexOf(`describe('${name}'`);
    if (start < 0) { throw new Error(`describe ${name} not found`); }
    const m = src.slice(start).match(/const data = (\[[\s\S]*?\n {8}\]);/);
    if (!m) { throw new Error(`data of ${name} not found`); }
    return new Function(`return ${m[1]};`)();
}

const furigana = dataOf('distributeFurigana').map(([[term, reading], expected]) => {
    const actual = jp.distributeFurigana(term, reading);
    if (JSON.stringify(actual) !== JSON.stringify(expected)) { throw new Error(`Yomitan 自己的用例不过？ ${term}`); }
    return {term, reading, expected};
});
const inflected = dataOf('distributeFuriganaInflected').map(([[term, reading, source], expected]) => {
    const actual = jp.distributeFuriganaInflected(term, reading, source);
    if (JSON.stringify(actual) !== JSON.stringify(expected)) { throw new Error(`Yomitan 自己的用例不过？ ${term}`); }
    return {term, reading, source, expected};
});

const extra = readFileSync(pairsPath, 'utf8').split(/\r?\n/).filter((l) => l.includes('\t')).map((l) => l.split('\t'));
const realFurigana = extra.map(([term, reading]) => ({term, reading, expected: jp.distributeFurigana(term, reading)}));
// 活用形：用词形本身和几种常见截断当 source，覆盖「源文本是词形 / 读音 / 片假名」三条分支
const realInflected = extra.slice(0, 400).flatMap(([term, reading]) => {
    const sources = [term, reading, jp.convertHiraganaToKatakana(reading), term.slice(0, -1) + 'た'];
    return sources.map((source) => ({term, reading, source, expected: jp.distributeFuriganaInflected(term, reading, source)}));
});

const morae = ['きょう', 'ジャンプ', 'しゃっきん', 'ぁ', '', 'ヴァイオリン', 'きゃーー'].map((text) => ({
    text,
    morae: jp.getKanaMorae(text),
    count: jp.getKanaMoraCount(text),
}));
const pitchValues = [0, 1, 2, 3, 5, 'LHHL', 'HLL', 'LHLH'];
const pitch = pitchValues.flatMap((value) => [0, 1, 2, 3, 4, 5].map((index) => ({index, value, high: jp.isMoraPitchHigh(index, value)})));
const downsteps = ['LHHL', 'HLL', 'LHH', 'HHH', 'LHLHL', ''].map((value) => ({value, positions: jp.getDownstepPositions(value)}));
const categories = [['はし', 0, false], ['はし', 1, false], ['はしら', 2, false], ['おとうと', 4, false], ['たべる', 2, true], ['いく', 0, true], ['はし', 'LHH', false]]
    .map(([text, value, verb]) => ({text, value, verb, category: jp.getPitchCategory(text, value, verb)}));
const diacritics = [...'がぎぐばぱヴゔかアハ'].map((character) => ({character, info: jp.getKanaDiacriticInfo(character)}));
const kanaConversion = ['ラーメン', 'コーヒー', 'ヵヶ', 'ーア', 'テスト', 'のー'].map((text) => ({text, hiragana: jp.convertKatakanaToHiragana(text)}));

const result = {
    source: 'yomitan d34832d756e05dc00945e5b7d7ebc80963299a7a',
    furigana: [...furigana, ...realFurigana],
    inflected: [...inflected, ...realInflected],
    morae,
    pitch,
    downsteps,
    categories,
    diacritics,
    kanaConversion,
};
writeFileSync(outPath, JSON.stringify(result));
console.log(JSON.stringify({
    furigana: result.furigana.length,
    yomitanFurigana: furigana.length,
    inflected: result.inflected.length,
    yomitanInflected: inflected.length,
    pitch: pitch.length,
}, null, 1));
