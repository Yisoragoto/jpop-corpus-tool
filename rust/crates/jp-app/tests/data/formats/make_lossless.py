"""从 tone.wav 生成 tone-alac.m4a、tone.aiff、tone.aif、tone-adpcm.wav。

其他夹具是 ffmpeg 生成的；这几种是手写的容器，不依赖 ffmpeg：

- ALAC 用「不压缩」帧（escape 帧，每个样本原样 16 位）。解码器走的是同一个入口，
  只是不经过预测那一段——这里要证明的是「认得出 ALAC、解得开」，不是测压缩算法。
- AIFF 是大端 PCM，.aif 和 .aiff 内容一样。
- ADPCM 是 IMA ADPCM（WAV 格式码 0x11），有损，所以测试只比时长和峰值。

用法：py -3 make_lossless.py（在本目录运行）。生成结果已提交，测试不需要运行它。
"""

import struct
import wave
from pathlib import Path

HERE = Path(__file__).resolve().parent


def read_tone():
    with wave.open(str(HERE / "tone.wav")) as w:
        assert (w.getnchannels(), w.getsampwidth()) == (1, 2)
        rate, n = w.getframerate(), w.getnframes()
        samples = list(struct.unpack(f"<{n}h", w.readframes(n)))
    return rate, samples


# ───────────────────────────── AIFF ─────────────────────────────


def extended80(value):
    """IEEE 754 80 位扩展精度（AIFF 的采样率字段），只处理正整数。"""
    exponent = value.bit_length() - 1
    mantissa = value << (63 - exponent)
    return struct.pack(">HQ", 16383 + exponent, mantissa)


def aiff(rate, samples):
    comm = struct.pack(">hIh", 1, len(samples), 16) + extended80(rate)
    ssnd = struct.pack(">II", 0, 0) + struct.pack(f">{len(samples)}h", *samples)
    body = b"AIFF" + chunk(b"COMM", comm) + chunk(b"SSND", ssnd)
    return chunk(b"FORM", body, big=True)


def chunk(tag, data, big=True):
    pad = b"\0" if len(data) % 2 else b""
    return tag + struct.pack(">I" if big else "<I", len(data)) + data + pad


# ───────────────────────────── IMA ADPCM WAV ─────────────────────────────

STEPS = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97,
    107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796,
    876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428,
    4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350,
    22385, 24623, 27086, 29794, 32767,
]
INDEX = [-1, -1, -1, -1, 2, 4, 6, 8]


def ima_encode_block(block, samples_per_block):
    predictor, index = block[0], 0
    out = struct.pack("<hBB", predictor, index, 0)
    rest = block[1:] + [block[-1]] * (samples_per_block - len(block))
    nibbles = []
    for s in rest:
        step = STEPS[index]
        diff = s - predictor
        code = 8 if diff < 0 else 0
        diff = abs(diff)
        delta, vpdiff = 4, 0
        for bit in (4, 2, 1):
            if diff >= step:
                code |= bit
                diff -= step
                vpdiff += step
            step >>= 1
        vpdiff += STEPS[index] >> 3
        predictor += -vpdiff if code & 8 else vpdiff
        predictor = max(-32768, min(32767, predictor))
        index = max(0, min(88, index + INDEX[code & 7]))
        nibbles.append(code)
    for lo, hi in zip(nibbles[0::2], nibbles[1::2]):
        out += bytes([lo | (hi << 4)])
    return out


def ima_wav(rate, samples):
    block_align = 512
    per_block = (block_align - 4) * 2 + 1
    data = b"".join(ima_encode_block(samples[i:i + per_block], per_block) for i in range(0, len(samples), per_block))
    avg = rate * block_align // per_block
    fmt = struct.pack("<HHIIHHHH", 0x11, 1, rate, avg, block_align, 4, 2, per_block)
    fact = struct.pack("<I", len(samples))
    body = b"WAVE" + chunk(b"fmt ", fmt, big=False) + chunk(b"fact", fact, big=False) + chunk(b"data", data, big=False)
    return chunk(b"RIFF", body, big=False)


# ───────────────────────────── ALAC in MP4 ─────────────────────────────


class Bits:
    def __init__(self):
        self.value, self.count = 0, 0

    def put(self, value, width):
        self.value = (self.value << width) | (value & ((1 << width) - 1))
        self.count += width

    def bytes(self):
        pad = -self.count % 8
        return (self.value << pad).to_bytes((self.count + pad) // 8, "big")


FRAME = 4096


def alac_frame(samples):
    b = Bits()
    b.put(0, 3)  # ID_SCE：单声道
    b.put(0, 4)  # element instance tag
    b.put(0, 12)  # unused
    partial = len(samples) != FRAME
    b.put((int(partial) << 3) | 1, 4)  # partialFrame、bytesShifted=0、escapeFlag=1
    if partial:
        b.put(len(samples), 32)
    for s in samples:
        b.put(s, 16)
    b.put(7, 3)  # ID_END
    return b.bytes()


def box(tag, *parts):
    data = b"".join(parts)
    return struct.pack(">I", 8 + len(data)) + tag + data


def full(tag, version_flags, *parts):
    return box(tag, struct.pack(">I", version_flags), *parts)


def alac_m4a(rate, samples):
    frames = [alac_frame(samples[i:i + FRAME]) for i in range(0, len(samples), FRAME)]
    sizes = [len(f) for f in frames]
    counts = [min(FRAME, len(samples) - i) for i in range(0, len(samples), FRAME)]
    n = len(samples)

    cookie = struct.pack(">IBBBBBBHIII", FRAME, 0, 16, 40, 10, 14, 1, 255, max(sizes), 0, rate)
    entry = box(
        b"alac",
        b"\0" * 6, struct.pack(">H", 1),  # reserved、data_reference_index
        struct.pack(">IIHHHHI", 0, 0, 1, 16, 0, 0, rate << 16),
        full(b"alac", 0, cookie),
    )
    stts_entries = [(counts.count(FRAME), FRAME)] + ([(1, counts[-1])] if counts[-1] != FRAME else [])
    stts = full(b"stts", 0, struct.pack(">I", len(stts_entries)), *(struct.pack(">II", c, d) for c, d in stts_entries))
    stsc = full(b"stsc", 0, struct.pack(">IIII", 1, 1, len(frames), 1))
    stsz = full(b"stsz", 0, struct.pack(">II", 0, len(sizes)), *(struct.pack(">I", s) for s in sizes))

    def moov(mdat_offset):
        stco = full(b"stco", 0, struct.pack(">II", 1, mdat_offset))
        stbl = box(b"stbl", full(b"stsd", 0, struct.pack(">I", 1), entry), stts, stsc, stsz, stco)
        dinf = box(b"dinf", full(b"dref", 0, struct.pack(">I", 1), full(b"url ", 1)))
        minf = box(b"minf", full(b"smhd", 0, struct.pack(">HH", 0, 0)), dinf, stbl)
        hdlr = full(b"hdlr", 0, struct.pack(">I", 0), b"soun", b"\0" * 12, b"SoundHandler\0")
        mdhd = full(b"mdhd", 0, struct.pack(">IIIIHH", 0, 0, rate, n, 0x55C4, 0))
        matrix = struct.pack(">9I", 0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000)
        tkhd = full(b"tkhd", 7, struct.pack(">IIIII", 0, 0, 1, 0, n * 1000 // rate), b"\0" * 8,
                    struct.pack(">HHHH", 0, 0, 0x0100, 0), matrix, struct.pack(">II", 0, 0))
        mvhd = full(b"mvhd", 0, struct.pack(">IIII", 0, 0, 1000, n * 1000 // rate), struct.pack(">IH", 0x10000, 0x0100),
                    b"\0" * 10, matrix, b"\0" * 24, struct.pack(">I", 2))
        return box(b"moov", mvhd, box(b"trak", tkhd, box(b"mdia", mdhd, hdlr, minf)))

    ftyp = box(b"ftyp", b"M4A ", struct.pack(">I", 0), b"M4A mp42isom\0\0\0\0")
    head = len(ftyp) + len(moov(0))
    mdat = box(b"mdat", *frames)
    return ftyp + moov(head + 8) + mdat


def main():
    rate, samples = read_tone()
    (HERE / "tone.aiff").write_bytes(aiff(rate, samples))
    (HERE / "tone.aif").write_bytes(aiff(rate, samples))
    (HERE / "tone-adpcm.wav").write_bytes(ima_wav(rate, samples))
    (HERE / "tone-alac.m4a").write_bytes(alac_m4a(rate, samples))


if __name__ == "__main__":
    main()
