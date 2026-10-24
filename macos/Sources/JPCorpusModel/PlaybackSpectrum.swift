import Accelerate
import AVFoundation
import Foundation

/// tap 线程独占 FFT 暂存区；锁只保护传给主线程的 48 条结果。
final class PlaybackSpectrum {
    private let count = 1024
    private let setup: FFTSetup?
    private var window = [Float](repeating: 0, count: 1024)
    private var samples = [Float](repeating: 0, count: 1024)
    private var input = [Float](repeating: 0, count: 1024)
    private var real = [Float](repeating: 0, count: 512)
    private var imag = [Float](repeating: 0, count: 512)
    private var cursor = 0
    private let lock = NSLock()
    private var bands = [Float](repeating: 0, count: 48)

    init() {
        setup = vDSP_create_fftsetup(vDSP_Length(10), FFTRadix(kFFTRadix2))
        vDSP_hann_window(&window, vDSP_Length(1024), Int32(vDSP_HANN_DENORM))
    }

    deinit { if let setup { vDSP_destroy_fftsetup(setup) } }

    func consume(_ buffer: AVAudioPCMBuffer) {
        guard let channels = buffer.floatChannelData, let setup else { return }
        let channelCount = Int(buffer.format.channelCount)
        let interleaved = buffer.format.isInterleaved
        guard channelCount > 0 else { return }
        for frame in 0..<Int(buffer.frameLength) {
            var mono: Float = 0
            for channel in 0..<channelCount {
                mono += interleaved ? channels[0][frame * channelCount + channel] : channels[channel][frame]
            }
            samples[cursor] = mono / Float(channelCount)
            cursor += 1
            if cursor < count { continue }
            cursor = 0
            vDSP_vmul(samples, 1, window, 1, &input, 1, vDSP_Length(count))
            input.withUnsafeBufferPointer { source in
                source.baseAddress!.withMemoryRebound(to: DSPComplex.self, capacity: 512) { packed in
                    real.withUnsafeMutableBufferPointer { realBuffer in
                        imag.withUnsafeMutableBufferPointer { imagBuffer in
                            var split = DSPSplitComplex(realp: realBuffer.baseAddress!, imagp: imagBuffer.baseAddress!)
                            vDSP_ctoz(packed, 2, &split, 1, vDSP_Length(512))
                            vDSP_fft_zrip(setup, &split, 1, vDSP_Length(10), FFTDirection(FFT_FORWARD))
                        }
                    }
                }
            }
            let nyquist = buffer.format.sampleRate / 2
            let binHz = nyquist / Double(count / 2)
            var previous = max(1, Int(30 / binHz))
            var result = [Float](repeating: 0, count: 48)
            for band in 0..<48 {
                let hz = 30 * pow(max(1, nyquist / 30), Double(band + 1) / 48)
                let end = min(count / 2, max(previous + 1, Int(hz / binHz)))
                var peak: Float = 0
                if previous < end {
                    for bin in previous..<end {
                        // 实数 FFT 自带 2 倍缩放，除以窗长得到单边幅度。
                        peak = max(peak, hypot(real[bin], imag[bin]) / Float(count))
                    }
                }
                result[band] = peak > 0 ? min(1, max(0, (20 * log10(peak) + 70) / 70)) : 0
                previous = end
            }
            lock.lock(); bands = result; lock.unlock()
        }
    }

    func snapshot() -> [Float] {
        lock.lock(); defer { lock.unlock() }
        return bands
    }

    func clear() {
        lock.lock(); bands = Array(repeating: 0, count: 48); lock.unlock()
    }
}
