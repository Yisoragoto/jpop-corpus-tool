import AVFoundation
import XCTest
@testable import JPCorpusModel

final class PlaybackSpectrumTests: XCTestCase {
    func testSilenceToneAndChannelCancellationWithoutAudioDevice() throws {
        let format = try XCTUnwrap(AVAudioFormat(standardFormatWithSampleRate: 48000, channels: 2))
        let buffer = try XCTUnwrap(AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 1024))
        let channels = try XCTUnwrap(buffer.floatChannelData)
        buffer.frameLength = 1024
        for index in 0..<1024 { channels[0][index] = 0; channels[1][index] = 0 }
        let spectrum = PlaybackSpectrum()
        spectrum.consume(buffer)
        XCTAssertEqual(spectrum.snapshot(), Array(repeating: 0, count: 48))
        for index in 0..<1024 {
            let value = Float(sin(2 * .pi * 440 * Double(index) / 48000))
            channels[0][index] = value; channels[1][index] = value
        }
        spectrum.consume(buffer)
        let bands = spectrum.snapshot()
        XCTAssertEqual(bands.count, 48)
        XCTAssertTrue(bands.allSatisfy { $0.isFinite && (0...1).contains($0) })
        XCTAssertGreaterThan(bands.max() ?? 0, 0.7)
        XCTAssertLessThan(bands.last ?? 1, 0.1)
        for index in 0..<1024 { channels[1][index] = -channels[0][index] }
        spectrum.consume(buffer)
        XCTAssertEqual(spectrum.snapshot(), Array(repeating: 0, count: 48))
        spectrum.clear()
        XCTAssertEqual(spectrum.snapshot().count, 48)
    }
}
