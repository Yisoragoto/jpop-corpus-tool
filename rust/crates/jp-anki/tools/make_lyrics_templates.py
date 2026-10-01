"""从 Lapis（GPL-3.0）的模板生成「Lyrics」笔记类型的模板。

只改两处：根元素加 class、背面右侧的图换成「封面 + 歌名/歌手/专辑」。
歌曲信息的样式不在这里生成，是手写的 `song-info.css`，程序把它接在 `styling.css` 后面。
其余原样保留，方便以后跟着 Lapis 更新：重新拉 Lapis、再跑一遍这个脚本即可。

用法: python make_lyrics_templates.py <Lapis 仓库根目录> <jp-anki/data/lyrics>
"""
import pathlib
import subprocess
import sys

lapis = pathlib.Path(sys.argv[1])
out = pathlib.Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)
commit = subprocess.run(['git', '-C', str(lapis), 'rev-parse', 'HEAD'], capture_output=True, text=True).stdout.strip()

notice_html = f'''<!--
  Lyrics 笔记类型（JPOP Corpus Tool）。改自 Lapis 笔记类型 https://github.com/donkuri/lapis
  （commit {commit}，GNU GPL v3.0）。本文件同样按 GPL-3.0-or-later 分发。
  改动：背面右侧的图用歌曲封面，封面旁显示歌名、歌手、专辑。
-->
'''
notice_css = f'''/*
 * Lyrics 笔记类型（JPOP Corpus Tool）。改自 Lapis 笔记类型 https://github.com/donkuri/lapis
 * （commit {commit}，GNU GPL v3.0）。本文件同样按 GPL-3.0-or-later 分发。
 * 用户设置（字号、图的位置等）仍在下面的 :root 里，说明见 Lapis 的 docs/user_settings.md。
 */
'''


def write(name, text):
    # 统一 LF：模板要和 Anki 里存的逐字比较，不能随检出方式变
    (out / name).write_bytes(text.replace('\r\n', '\n').encode('utf-8'))


def replace_once(text, old, new):
    assert text.count(old) == 1, old[:60]
    return text.replace(old, new)


front = (lapis / 'src/front.html').read_text(encoding='utf-8')
front = replace_once(front, '<div id="lapis">', '<div id="lapis" class="lyrics">')
write('front.html', notice_html + front)

back = (lapis / 'src/back.html').read_text(encoding='utf-8')
back = replace_once(back, '<div id="lapis" lang="ja">', '<div id="lapis" class="lyrics" lang="ja">')
song_info = '''<div class="song-info">
                    {{#SongTitle}}<div class="song-title">{{SongTitle}}</div>{{/SongTitle}}
                    {{#Artist}}<div class="song-artist">{{Artist}}</div>{{/Artist}}
                    {{#Album}}<div class="song-album">{{Album}}</div>{{/Album}}
                </div>'''
back = replace_once(back, '''            <!-- Image -->
            {{#Picture}}
            <div class="dh-image">
                <div class="image tappable {{Tags}}">{{Picture}}</div>
            </div>
            {{/Picture}}''', f'''            <!-- 歌曲：封面，旁边是歌名、歌手、专辑 -->
            {{{{#Picture}}}}
            <div class="dh-image dh-song">
                {song_info}
                <div class="image tappable {{{{Tags}}}}">{{{{Picture}}}}</div>
            </div>
            {{{{/Picture}}}}
            {{{{^Picture}}}}{{{{#SongTitle}}}}
            <div class="dh-song dh-song-text">
                {song_info}
            </div>
            {{{{/SongTitle}}}}{{{{/Picture}}}}''')
write('back.html', notice_html + back)

css = (lapis / 'src/styling.css').read_text(encoding='utf-8')
write('styling.css', notice_css + css + '\n')
write('LAPIS_COMMIT', commit + '\n')
print('ok', commit, sorted(p.name for p in out.iterdir()))
