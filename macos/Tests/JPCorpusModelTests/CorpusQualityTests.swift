import XCTest
import JPCore
@testable import JPCorpusModel

/// corpusQuality.test.ts 的其余 3 例；条件的 5 例见 SubcorpusScopeTests。
final class CorpusQualityTests: XCTestCase {
    func testEveryValidatorIssueHasLabel() {
        let codes = ["empty","replacement_char","not_japanese","too_few_lines","ends_early","untimed","mixed_timing",
                     "non_monotonic","beyond_duration","duplicate_timestamp","stacked_duplicate","long_gap","leftover_markup","very_long_line"]
        XCTAssertEqual(Set(CorpusQuality.issueLabels.keys),Set(codes))
        XCTAssertEqual(CorpusQuality.issueLabel("future"),"future")
    }
    func testCompletenessGrades() {
        XCTAssertEqual(CorpusQuality.completenessLabel(nil),"未校验")
        for (value,label) in [(1.0,"完整"),(0.85,"基本完整"),(0.6,"可能有缺"),(0.2,"很可能有问题")] {
            XCTAssertEqual(CorpusQuality.completenessLabel(value),label)
        }
    }
    func testMetricsAndTokenizerNames() throws {
        XCTAssertEqual(CorpusQuality.metricLabel("ttr"),"TTR")
        XCTAssertEqual(CorpusQuality.metricLabel("coverage@500"),"Top 500 覆盖率")
        let run = try VisualFixture.decode(LyricTokenizerRun.self,["id":1,"fingerprint":"x","engine":"sudachi.rs","engineVersion":"0.6.11",
            "dictDescription":"","dictCreatedAt":"","dictSize":0,"splitMode":"C","configHash":"","posScheme":"upos-1","lineCount":0])
        XCTAssertEqual(CorpusQuality.tokenizerLabel(run),"sudachi.rs 0.6.11 · 未知词典 · Mode C")
    }
    func testMetricSidesAndPercentFormatting() {
        XCTAssertEqual(CorpusQuality.metricLabel("target.tokens"),"目标词次")
        XCTAssertEqual(CorpusQuality.metricLabel("provenance.songs"),"曲数")
        XCTAssertEqual(CorpusQuality.metricLabel("future"),"future")
        XCTAssertEqual(CorpusQuality.percent(nil),"—")
        XCTAssertEqual(CorpusQuality.percent(0.805),"81%")
        XCTAssertEqual(CorpusQuality.percent(0.854,digits: 1),"85.4%")
    }
}
