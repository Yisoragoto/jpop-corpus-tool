//! 采样 tap：把流过播放链的样本抄一份到环形缓冲，供频谱使用。
//!
//! **音频线程绝不为可视化阻塞。** 写入用 `try_lock`，拿不到锁就直接丢掉这批
//! 样本——频谱掉一帧没人看得出来，音频卡一下所有人都听得出来。
//!
//! 这是整个模块唯一需要小心的地方：这段代码跑在实时音频回调里。

use std::sync::{Arc, Mutex};

use rodio::{ChannelCount, SampleRate, Source};

/// 环形缓冲。容量取 2 的幂，取模退化成位与。
pub struct TapBuffer {
    inner: Mutex<Ring>,
    capacity: usize,
}

struct Ring {
    data: Box<[f32]>,
    /// 下一个写入位置
    write: usize,
    /// 累计写入量，用来判断缓冲是否已经填满过
    written: u64,
}

impl TapBuffer {
    /// `capacity` 会向上取到 2 的幂。频谱窗口通常 1024~4096。
    pub fn new(capacity: usize) -> Arc<Self> {
        let capacity = capacity.next_power_of_two().max(64);
        Arc::new(Self {
            inner: Mutex::new(Ring {
                data: vec![0.0; capacity].into_boxed_slice(),
                write: 0,
                written: 0,
            }),
            capacity,
        })
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 从音频线程写入。拿不到锁就丢弃——见模块注释。
    pub fn push(&self, sample: f32) {
        if let Ok(mut ring) = self.inner.try_lock() {
            let mask = self.capacity - 1;
            let slot = ring.write & mask;
            ring.data[slot] = sample;
            ring.write = ring.write.wrapping_add(1);
            ring.written = ring.written.saturating_add(1);
        }
    }

    /// 从 UI 线程读最近的 `len` 个样本，按时间顺序排列。
    ///
    /// 还没写满时前面补零——这样频谱在开播瞬间是安静的，
    /// 而不是拿一堆未初始化的旧数据算出一片噪声。
    pub fn latest(&self, len: usize) -> Vec<f32> {
        let len = len.min(self.capacity);
        let mut out = vec![0.0; len];
        let Ok(ring) = self.inner.lock() else {
            return out;
        };
        let available = (ring.written as usize).min(self.capacity);
        let take = len.min(available);
        let mask = self.capacity - 1;
        for i in 0..take {
            // 从最新往回数，写进 out 的尾部
            let src = ring.write.wrapping_sub(take - i) & mask;
            out[len - take + i] = ring.data[src];
        }
        out
    }

    /// 有没有攒够数据。UI 可以据此决定要不要画频谱。
    pub fn is_primed(&self, len: usize) -> bool {
        self.inner
            .lock()
            .map(|r| (r.written as usize) >= len)
            .unwrap_or(false)
    }

    #[cfg(test)]
    fn written(&self) -> u64 {
        self.inner.lock().map(|r| r.written).unwrap_or(0)
    }
}

/// 把 tap 插进 rodio 的 Source 链。透明转发，只是顺手抄一份样本。
pub struct Tapped<S> {
    inner: S,
    tap: Arc<TapBuffer>,
}

impl<S> Tapped<S> {
    pub fn new(inner: S, tap: Arc<TapBuffer>) -> Self {
        Self { inner, tap }
    }

    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S> Iterator for Tapped<S>
where
    S: Source,
{
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.inner.next()?;
        self.tap.push(sample);
        Some(sample)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S> Source for Tapped<S>
where
    S: Source,
{
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, pos: std::time::Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_rounds_up_to_a_power_of_two() {
        assert_eq!(TapBuffer::new(1000).capacity(), 1024);
        assert_eq!(TapBuffer::new(1024).capacity(), 1024);
        // 太小的容量没有意义，抬到下限
        assert_eq!(TapBuffer::new(1).capacity(), 64);
    }

    #[test]
    fn empty_buffer_reads_as_silence() {
        let tap = TapBuffer::new(64);
        assert_eq!(tap.latest(16), vec![0.0; 16]);
        assert!(!tap.is_primed(1));
    }

    #[test]
    fn latest_returns_samples_in_chronological_order() {
        let tap = TapBuffer::new(64);
        for i in 0..8 {
            tap.push(i as f32);
        }
        // 最近 4 个应当是 4,5,6,7 —— 顺序不能反
        assert_eq!(tap.latest(4), vec![4.0, 5.0, 6.0, 7.0]);
    }

    #[test]
    fn partial_fill_is_zero_padded_at_the_front() {
        let tap = TapBuffer::new(64);
        tap.push(1.0);
        tap.push(2.0);
        // 只写了 2 个却要 5 个：前面补零，新数据在尾部
        assert_eq!(tap.latest(5), vec![0.0, 0.0, 0.0, 1.0, 2.0]);
    }

    #[test]
    fn ring_wraps_and_keeps_only_the_newest() {
        let tap = TapBuffer::new(64);
        for i in 0..200 {
            tap.push(i as f32);
        }
        let latest = tap.latest(4);
        assert_eq!(latest, vec![196.0, 197.0, 198.0, 199.0]);
    }

    #[test]
    fn asking_for_more_than_capacity_is_clamped() {
        let tap = TapBuffer::new(64);
        for i in 0..100 {
            tap.push(i as f32);
        }
        assert_eq!(tap.latest(10_000).len(), tap.capacity());
    }

    #[test]
    fn is_primed_tracks_total_writes() {
        let tap = TapBuffer::new(64);
        assert!(!tap.is_primed(4));
        for i in 0..4 {
            tap.push(i as f32);
        }
        assert!(tap.is_primed(4));
        assert!(!tap.is_primed(5));
    }

    #[test]
    fn tapped_source_forwards_samples_unchanged() {
        use rodio::source::SineWave;

        let tap = TapBuffer::new(1024);
        let mut plain = SineWave::new(440.0);
        let mut tapped = Tapped::new(SineWave::new(440.0), Arc::clone(&tap));

        for _ in 0..256 {
            assert_eq!(tapped.next(), plain.next(), "tap 不该改变样本");
        }
        assert_eq!(tap.written(), 256);
    }

    #[test]
    fn tapped_source_preserves_format() {
        use rodio::source::SineWave;

        let tap = TapBuffer::new(64);
        let plain = SineWave::new(440.0);
        let channels = plain.channels();
        let rate = plain.sample_rate();
        let tapped = Tapped::new(SineWave::new(440.0), tap);
        assert_eq!(tapped.channels(), channels);
        assert_eq!(tapped.sample_rate(), rate);
    }
}
