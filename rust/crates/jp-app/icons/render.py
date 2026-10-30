"""把 icons/src 里的两张 SVG 渲染成 Tauri 用的 PNG 和 icon.ico。

    py -3 rust/crates/jp-app/icons/render.py

需要本机装了 Chrome（或 Edge，路径改 BROWSER）和 Pillow。SVG 先用无头浏览器渲染成 1024×1024（背景透明），
再缩到各尺寸：16–48 用 icon-small.svg（细节少、音符粗，任务栏上看得清），64 以上用 icon.svg。
icon.ico 里每个尺寸都是一张 PNG（Vista 起支持），不让 Windows 从一张大图临时缩放。
"""
import io, os, pathlib, shutil, struct, subprocess, tempfile

from PIL import Image

HERE = pathlib.Path(__file__).resolve().parent
BROWSER = next((p for p in [
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    shutil.which("chrome") or "", shutil.which("chromium") or "",
] if p and os.path.exists(p)), None)

SMALL = [16, 24, 32, 48]
LARGE = [64, 128, 256]


def render(svg: pathlib.Path) -> Image.Image:
    with tempfile.TemporaryDirectory() as tmp:
        tmp = pathlib.Path(tmp)
        page = tmp / "page.html"
        page.write_text(f'<html><body style="margin:0;background:transparent">'
                        f'<img src="{svg.as_uri()}" width="1024" height="1024" style="display:block"></body></html>',
                        encoding="utf-8")
        shot = tmp / "shot.png"
        subprocess.run([BROWSER, "--headless=new", "--disable-gpu", "--hide-scrollbars",
                        f"--user-data-dir={tmp / 'profile'}", "--default-background-color=00000000",
                        "--window-size=1024,1024", "--allow-file-access-from-files",
                        f"--screenshot={shot}", page.as_uri()],
                       check=True, capture_output=True)
        return Image.open(shot).convert("RGBA").copy()


def ico(images: list[Image.Image]) -> bytes:
    blobs = []
    for im in images:
        buf = io.BytesIO(); im.save(buf, "PNG"); blobs.append(buf.getvalue())
    head = struct.pack("<HHH", 0, 1, len(images))
    offset = len(head) + 16 * len(images)
    entries = b""
    for im, blob in zip(images, blobs):
        w, h = im.size
        entries += struct.pack("<BBBBHHII", w % 256, h % 256, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
    return head + entries + b"".join(blobs)


def main():
    assert BROWSER, "找不到 Chrome / Edge"
    small = render(HERE / "src/icon-small.svg")
    large = render(HERE / "src/icon.svg")
    size = lambda src, n: src.resize((n, n), Image.LANCZOS)
    size(small, 32).save(HERE / "32x32.png")
    size(large, 128).save(HERE / "128x128.png")
    size(large, 256).save(HERE / "256x256.png")
    size(large, 512).save(HERE / "icon.png")
    (HERE / "icon.ico").write_bytes(ico([size(small, n) for n in SMALL] + [size(large, n) for n in LARGE]))
    # 预览：各尺寸并排，方便肉眼检查小尺寸是否清楚
    sheet = Image.new("RGBA", (16 + sum(n + 16 for n in SMALL + LARGE), 256 + 32), (243, 243, 243, 255))
    x = 16
    for n in SMALL + LARGE:
        sheet.alpha_composite(size(small if n in SMALL else large, n), (x, 16 + 256 - n)); x += n + 16
    preview = pathlib.Path(os.environ.get("ICON_PREVIEW", tempfile.gettempdir())) / "jpop-icon-preview.png"
    sheet.save(preview)
    print("ok ->", HERE, "preview:", preview)


main()
