// 从 Yomitan 源码导出日语活用规则 + 测试用例 + 全量输出，给 Rust 移植对账用。
// 用法: node dump-ja-transforms.mjs <yomitan 根目录> <输出目录>
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {pathToFileURL} from 'node:url';
import {join} from 'node:path';

const [root, outDir] = process.argv.slice(2);
mkdirSync(outDir, {recursive: true});
const imp = (p) => import(pathToFileURL(join(root, p)).href);
const {japaneseTransforms} = await imp('ext/js/language/ja/japanese-transforms.js');
const {LanguageTransformer} = await imp('ext/js/language/language-transformer.js');

const META = /[\\^$.*+?()[\]{}|]/;
const warnings = [];
let suffix = 0, whole = 0;

function ruleOut(r, where) {
    const {source, flags} = r.isInflected;
    if (flags !== '') { throw new Error(`${where}: regex flags ${flags}`); }
    if (r.type === 'suffix') {
        if (!source.endsWith('$')) { throw new Error(`${where}: ${source}`); }
        const inflected = source.slice(0, -1);
        if (META.test(inflected)) { throw new Error(`${where}: regex meta in ${source}`); }
        if (inflected.length === 0) { warnings.push(`${where}: empty inflected suffix`); }
        const probe = 'ＸＹＺ' + inflected;
        if (inflected.length > 0 && r.deinflect(probe) !== 'ＸＹＺ' + r.deinflected) {
            throw new Error(`${where}: deinflect mismatch`);
        }
        ++suffix;
        return {type: 'suffix', inflected, deinflected: r.deinflected, conditionsIn: r.conditionsIn, conditionsOut: r.conditionsOut};
    }
    if (r.type === 'wholeWord') {
        if (!source.startsWith('^') || !source.endsWith('$')) { throw new Error(`${where}: ${source}`); }
        const inflected = source.slice(1, -1);
        if (META.test(inflected)) { throw new Error(`${where}: regex meta in ${source}`); }
        ++whole;
        return {type: 'wholeWord', inflected, deinflected: r.deinflect(''), conditionsIn: r.conditionsIn, conditionsOut: r.conditionsOut};
    }
    throw new Error(`${where}: unsupported rule type ${r.type}`);
}

const data = {
    language: japaneseTransforms.language,
    conditions: Object.entries(japaneseTransforms.conditions).map(([id, c]) => ({
        id,
        name: c.name,
        isDictionaryForm: c.isDictionaryForm,
        subConditions: c.subConditions ?? null,
    })),
    transforms: Object.entries(japaneseTransforms.transforms).map(([id, t]) => ({
        id,
        name: t.name,
        description: t.description ?? null,
        rules: t.rules.map((r, j) => ruleOut(r, `${id}[${j}]`)),
    })),
};
const extraKeys = new Set();
for (const t of Object.values(japaneseTransforms.transforms)) {
    for (const k of Object.keys(t)) { if (!['name', 'description', 'rules'].includes(k)) { extraKeys.add(k); } }
}
for (const c of Object.values(japaneseTransforms.conditions)) {
    for (const k of Object.keys(c)) { if (!['name', 'isDictionaryForm', 'subConditions'].includes(k)) { extraKeys.add('condition.' + k); } }
}
writeFileSync(join(outDir, 'japanese-transforms.json'), JSON.stringify(data, null, 1));

// 测试用例：原样取 test 文件里的 const tests = [...]
const testSrc = readFileSync(join(root, 'test/language/japanese-transforms.test.js'), 'utf8');
const m = testSrc.match(/const tests = (\[[\s\S]*?\n\]);/);
if (!m) { throw new Error('tests array not found'); }
const tests = new Function('return ' + m[1])();
writeFileSync(join(outDir, 'japanese-transforms-tests.json'), JSON.stringify(tests, null, 1));

// 全量输出：每个测试 source 的 transform() 结果，逐项 text/conditions/trace
const lt = new LanguageTransformer();
lt.addDescriptor(japaneseTransforms);
const sources = [...new Set(tests.flatMap((c) => c.tests.map((t) => t.source)))];
const extra = readFileSync(join(outDir, 'extra-sources.txt'), 'utf8').split(/\r?\n/).filter((s) => s.length > 0);
const all = [...new Set([...sources, ...extra])];
const full = all.map((source) => [source, lt.transform(source).map(({text, conditions, trace}) =>
    `${text}\t${conditions}\t${trace.map((f) => `${f.transform}:${f.ruleIndex}`).join(',')}`)]);
writeFileSync(join(outDir, 'japanese-transforms-full.json'), JSON.stringify(full));

const flags = {};
for (const {id} of data.conditions) { flags[id] = lt.getConditionFlagsFromConditionType(id); }
writeFileSync(join(outDir, 'japanese-condition-flags.json'), JSON.stringify(flags, null, 1));

const caseCount = tests.reduce((n, c) => n + c.tests.length, 0);
const resultCount = full.reduce((n, [, r]) => n + r.length, 0);
console.log(JSON.stringify({
    conditions: data.conditions.length, transforms: data.transforms.length, suffix, whole,
    testCategories: tests.length, testCases: caseCount, sources: all.length, results: resultCount,
    extraKeys: [...extraKeys], warnings,
}, null, 1));
