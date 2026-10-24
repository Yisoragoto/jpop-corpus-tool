<div align="center">

# JPOP Corpus Tool

[![license](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSE)
[![platform](https://img.shields.io/badge/platform-Windows%2010%2F11%20%C2%B7%20macOS%2014%2B-lightgrey)](https://github.com/Yisoragoto/jpop-corpus-tool/releases)
[![built with](https://img.shields.io/badge/built%20with-Tauri%202%20%C2%B7%20React%2019%20%C2%B7%20Rust%20%C2%B7%20SwiftUI-orange)](#development)

A local J-pop corpus for Japanese learners. Import songs and LRC lyrics you own, read them line by line
along with the audio, look words up Yomitan-style, search a word across your whole library and hear the line
it is sung in, run corpus analyses — frequency, collocations, sub-corpus comparison — and turn the lines you
just heard into Anki cards.

[简体中文](README.md) · **English**

<table>
<tr>
<td><img src="docs/screenshots/lyrics-fullscreen.webp" alt="Full-screen lyrics"></td>
<td><img src="docs/screenshots/library.webp" alt="Library"></td>
</tr>
<tr>
<td><img src="docs/screenshots/dictionary.webp" alt="Dictionaries"></td>
<td><img src="docs/screenshots/kwic.webp" alt="KWIC search"></td>
</tr>
<tr>
<td><img src="docs/screenshots/anki-page.webp" alt="Anki page"></td>
<td><img src="docs/screenshots/scrape.webp" alt="Scraping page"></td>
</tr>
</table>

</div>

> This repository ships source code and build tooling only. It contains no commercial songs, lyrics,
> third-party dictionaries, Anki decks or pre-built corpus database.

## Download

**Windows**: grab the latest `JPOP.Corpus.Tool_x.y.z_x64-setup.exe` (or the `.msi`) from
[Releases](https://github.com/Yisoragoto/jpop-corpus-tool/releases). Windows 10/11 needs the
**WebView2 runtime**, which the system usually already has and the installer offers to add if not.

The interface comes in **English and Simplified Chinese**: pick one on first launch, and switch any time
under **Settings → Appearance**.

When a new release is out, a dialog shows what changed and lets you choose **Update now**, **Later** or
**Skip this version**. The installer is pulled from this repository's Releases, its SHA-256 is verified,
and it installs over the current version (no uninstall step) and reopens the app.

**macOS**: a native SwiftUI client lives in `macos/`; for now it is built from source — see
[Native macOS client](#native-macos-client). Both clients share one Rust core and one library format, so a
library folder copied over from Windows opens as is.

The old PyQt6 Python code is kept under `legacy/` for reference; `corpus.db` works with it too.

## Features

### Library and playback

- One library page for songs and people, browsed four ways: **musicians, behind the scenes, albums, songs**.
  - Band members are nested under their band, former members with their years; a member who went solo gets their
    own profile, with solo work kept apart from the band years (the band's songs stay the band's).
  - Memberships come from scraping (MusicBrainz), from suggestions based on credits that you confirm, or from
    manual edits; whatever you change or reject is never overwritten by a scrape.
  - The sidebar collapses to give the content the full width.
- A profile for every person, laid out like an artist page in a music app: a large photo and name, the roles they
  had and the bands they were in, then four tabs — overview, works (track lists by album), members & collaborators,
  and a release axis. Artist photos can be
  replaced by hand (scraping won't overwrite them, and the scraped one can be restored).
- Songs have a breadcrumb (artist › album › title) and clickable credits; edit, replace lyrics, line-by-line
  lyric editing, source & validation, and delete live in the "⋯" menu.

The **relationship graph** follows Obsidian's graph view: everyone and their collaborations as one network. Node
colour is the role (vocals, music, lyrics, arrangement), people with a photo show it, and node size follows how
many songs they are on; solid lines are shared credits, dashed lines are band membership. Drag, zoom, hover to
highlight neighbours, filter by role and by "at least N songs together", show only one person's neighbourhood,
and double-click to open a profile.

![Relationship graph](docs/screenshots/graph.webp)

The **timeline** gives each musician a lane with releases laid out by date, each node being the album or single
cover; crowded spots are staggered or folded into "+N", and where a band split, a dashed line leads to the
members' solo lanes. Click a name for that person's zoomed time tree, horizontal or vertical.

The **word cloud** sits on frosted glass over blurred artist photos. Its scope can be the whole library, one
artist or a sub-corpus; part of speech (nouns by default), Japanese-only, word count and font are all adjustable.
Click a word to list the songs it appears in; click a song and it **plays from that line in full-screen lyrics**,
returning to the cloud when you close them. The playing line shows as a caption under the cloud; there is a
full-screen mode and PNG export.

<table>
<tr>
<td><img src="docs/screenshots/timeline.webp" alt="Timeline"></td>
<td><img src="docs/screenshots/wordcloud.webp" alt="Word cloud"></td>
</tr>
</table>

Playback:

- Lyrics follow playback line by line; click a line to jump there, or loop a single line. Where a line starts and
  ends in the audio follows one rule everywhere (until the next line starts; instrumental breaks are estimated from
  the line's length), so looping the line before a break doesn't loop the break with it.
- A player bar modelled on Apple Music: frosted glass, ±5 s skips, speed and key in one panel, remaining / total
  time on a click.
- Speed changes go through WSOLA time-stretching (**pitch stays put**); key changes are rendered once per song by
  the built-in Rubber Band library and cached.
- Full-screen lyrics with the cover art blurred to fill the background, a side card listing vocals, lyrics, music
  and arrangement, and the player floating on top as frosted glass.
- Furigana over kanji only or over whole words; font, size, line height and letter spacing are all adjustable.
- F11 puts the whole app in full screen; layouts are tuned for narrow windows and for 27-inch and larger screens.

### Word lookup (Yomitan dictionaries)

[Yomitan](https://github.com/yomidevs/yomitan) is the browser pop-up dictionary for Japanese (the successor to
Yomichan). It defines a dictionary package format: one zip holding terms, reading-aware frequencies, pitch accent
and structured glossaries. **Lookup here is a port of what Yomitan does, to Rust**, so it eats those dictionaries
directly:

- Import Yomitan zips on the Dictionaries page, as many as you like, and drag them into the order results follow.
- Inflected forms, katakana and romaji all resolve; matching runs from the clicked word to the end of the line,
  so clicking the first word of 打ち込んでいませんでした gets you back to 打ち込む.
- Glossaries keep their original structure (nesting, examples, images); pitch and frequency are shown per reading.
- Clicking a word in the library puts **Dictionary** and **Examples** side by side: the glossary, and every line
  in your own corpus where that word appears, each with ▶ to hear it.

**Dictionaries are not included.** [MarvNC/yomitan-dictionaries](https://github.com/MarvNC/yomitan-dictionaries)
collects download links for the usual ones (JMdict, 三省堂, 明鏡, JPDB frequency, pitch-accent dictionaries…).
Each dictionary's licence and redistribution terms are its author's; check them yourself.

![Lookup inside full-screen lyrics](docs/screenshots/lyrics-lookup.webp)

### Search: from a word to the line it is sung in

- KWIC concordance with the keyword centred. Match by written form or by dictionary form (駆ける / 駆けた / 駆けろ
  together); filter by artist, year, part of speech, cross-line matches or Japanese-only, or restrict it to a
  sub-corpus; export CSV.
- Multi-word expressions are written with `+` (`て+しまう`); a "near" filter keeps only lines where another word
  occurs within a few words.
- Identical lines within one song (choruses) are folded into one hit by default, with the repeat count shown.
- **Search results are wired to the player**: ▶ plays just that line (stops at the line end), ⟲ adds one line of
  context on each side, and clicking a hit opens the song at that line and keeps playing with the lyrics
  highlighted; the hit that's playing is highlighted too. Precision is per line (LRC timestamps; no word timing).
- A collocation panel lists words within 3 words of the keyword; pick one to narrow the concordance to lines
  where it occurs nearby.

### Analysis: choruses don't inflate the numbers, and results say where they came from

J-pop repeats its choruses: in a real library, repeated lines make up about 23 % of lines and 20 % of tokens.
Counted naively, a good part of a frequency list is chorus. So every statistic on the Analytics page can count
**every token**, or count **each repeated line within a song once**.

- **Frequency**: lemma × part of speech with JLPT level; both counts side by side, plus the share coming from
  repeated lines.
- **Collocations**: logDice, MI, t-score and log-likelihood (2×2 contingency, expectations from the actual window
  positions).
- **Chorus effect**: the same songs counted both ways, showing which words were pushed up the list by choruses.
- **Sub-corpus comparison**: pick two sets of songs (one artist against the rest, the 2010s against the 2020s…)
  and find the words one side uses markedly more or less, by signed log-likelihood and log ratio.
- **Song profile**: how much of a song is repeated, and its characteristic words compared with the other songs
  (content words only); in the Library's right pane, click a word to search it.
- Frequency table, a timeline by year, and a corpus report as TXT.

**Sub-corpora**: slice by artist, year, album, genre, lyric language, chosen songs and minimum lyric completeness —
conditions are ANDed, multiple values within one are ORed. Save them under a name and use them directly in KWIC,
the word cloud, examples, frequency, collocations and comparisons.

**Every result carries its provenance**: scope, counting unit, rule version, and a fingerprint of the library at
the time (line and token counts, corrections, a hash of the metadata). Same fingerprint and parameters, same
result; importing songs or correcting tokenisation changes the fingerprint, so it's obvious when two numbers came
from different libraries.

**Reproducible analyses**: archiving an analysis stores the filter, every song in scope with its lyric version,
the tokenizer (engine, dictionary, split mode, config fingerprint), the counting rules, the app and schema
versions, and the time. Re-run it later on the current library: it is saved as a new run and compared item by
item, with an explanation of why the result changed — the library changed (lyrics replaced, tokens corrected,
songs added) or the counting rules did.

![Analytics page](docs/screenshots/analytics.webp)

### Corpus quality and provenance

A corpus is pieced together from many places: lyrics from an `.lrc` next to the audio, from an online search, or
pasted in by hand. To know what each number stands on:

- **Every set of lyrics is kept as a version**: its source (file / embedded / online / manual), import time, the
  original text verbatim and a content fingerprint. Replacing lyrics adds a version; old ones can be viewed and
  restored.
- **Automatic validation**: missing or truncated lines, pasted duplicates, timestamps going backwards or past the
  end of the song, long gaps, mojibake, leftover timestamps and credits, lyrics not in Japanese; plus an estimated
  completeness. Each line has a timestamp confidence and a tokenisation status (auto / corrected / pending / empty).
- **Line-by-line lyric editing**: fix text, add or remove lines, move timestamps; saved as a new version,
  re-tokenised, with your token corrections re-applied by text.
- **Revision history**: replaced lyrics, token corrections and reverts, re-tokenisation, metadata edits and
  deletions are all recorded.
- **Field provenance**: whether each field came from tags, the file name, the folder, scraping (which source, what
  match score) or your own edit; manual edits are never overwritten by automatic steps. Artist details record
  their source and confidence as well.
- A corpus-quality card on the Analytics page sums up which songs' lyrics look suspicious and what's missing.

Design notes: [docs/provenance.md](docs/provenance.md) (Chinese).

### Anki mining

[Anki](https://apps.ankiweb.net/) is the open-source spaced-repetition app;
[AnkiConnect](https://ankiweb.net/shared/info/2055492159) is the add-on that opens a local port for other programs
to add cards through. That is how cards get made here — **local only, nothing is uploaded**.

- Press **＋ Mine** on a lookup result: word, glossary, frequency, pitch and a real example go into Anki together.
- Ships a **Lyrics** note type adapted from [Lapis](https://github.com/donkuri/lapis): cover art on the right of the
  back side, with title, artist and album set vertically beside it. Created on first use if Anki doesn't have it.
- The example is the line you clicked, followed by the other lines in that song containing the word — **each with its
  own audio clip cut from the song**.
- Optional per-artist subdecks; reads Anki's learning state so words you already know can be left out.
- A mining report (HTML) lists, by JLPT level, the words in your corpus that have no card yet.

![The back of a Lyrics card](docs/screenshots/anki-lyrics-card.webp)

### Importing and metadata

- Import a folder **or individual songs**; scanning is read-only, nothing is written until you have reviewed the plan;
  **the original metadata in your files is never overwritten**. The plan marks each file as new, already in the
  library, a possible duplicate (not imported — left for you to look at) or unimportable, and shows whether the
  artist came from tags, the file name or the folder name.
- Drop new audio into a folder your library already uses and the app notices on the next start: a banner lists the
  new songs and imports them in one click (or sends you to the plan when a file's tags can't be read). The check is
  read-only, never imports on its own, and the **Import** page shows which folders it looks at.
- Lyrics: an `.lrc` sitting next to the audio, or lyrics embedded in the audio file, are picked up on import. When
  there are none —
  - **Settings → Library maintenance → Fill in missing lyrics** does the whole library: it looks next to the audio
    first, then searches online (NetEase).
  - Per song, the library (the "⋯" next to the title) offers **Fetch online** and **Import a lyrics file…**
    (`.lrc` / `.txt`; UTF-8, Shift-JIS and GBK are all read).
  - **No guessing**: a candidate is accepted only if title, artist and duration all line up — too many songs share a
    title, and the wrong lyrics are worse than none. Whatever can't be matched is left for you to supply.
  - Fetched lyrics land in `raw/lyrics_lrc/{song_id}.lrc` beside the imported ones, and tokenisation you corrected
    by hand is re-applied by text, so it survives replacing the lyrics.
- The Scrape page is a library-completeness checklist: identification, track covers, album covers, artist photos,
  lyrics and durations each get a row with a progress bar (how many are complete out of how many), and the row that
  has gaps carries a button saying exactly what it will do ("fill 1 cover", "scrape 3 artists").
  - Tracks are identified through iTunes → MusicBrainz, covers come from Cover Art Archive, artist photos from
    Deezer. Anything below the confidence threshold stops in a review queue for a human.
  - Scraping an artist also picks up band members, the date a band split and full release dates (when MusicBrainz
    has more than one artist by that name, it declines to guess rather than attach someone else's members).
- Already have a library somewhere else? **Settings → Library maintenance → Migrate from another library
  directory** previews what would move, then **merges** songs, lyrics (with their versions and provenance),
  tokenisation, credits, play history, sub-corpora and dictionaries into the current library. Songs you already
  have are skipped, and audio files are not copied — they keep playing from where they are.

### Native macOS client

`macos/` is a native SwiftUI client (playback through AVFoundation) with **no business logic of its own**: search,
analysis, line spans, import, tokenisation, lookup, the schema and its migrations all come from the same Rust core
(through `jp-ffi`: one entry point, JSON in and out, method names matching the Windows app's commands).

- Open a library copied from Windows: when it stores paths like `E:\...`, audio is found by file name in folders
  you point it at.
- Or build one from scratch in `~/Library/Application Support/JPOP Corpus Tool`: pick a music folder → review the
  plan → import with progress (and stop midway if you like); the tokenizer dictionary downloads from the Import
  page (the same file and checksum as on Windows).
- Home: continue listening (down to the line you stopped at), today's listening, how much of the vocabulary the songs
  you have heard cover, recently played, favourites, recently added; ⌘K searches songs, albums, people, words and lyrics.
- Library: a song table, an album wall and an artist wall, with album / artist / song pages (credits, collaborators,
  corpus profile, lyric sources and versions); favourites are shared with the Windows app.
- Search → KWIC → Space to hear the line / Return to open the song, with the lyrics inspector highlighting along;
  single-line loop and speed control (pitch preserved).
- Analysis: chorus-aware frequency, the collocation panel, sub-corpus comparison, song profiles; scope them to an
  artist and a range of years; archive a run and re-run it later to see what changed (the same archive table as on
  Windows); a corpus-quality overview.
- Right-click a word in the lyrics panel to search it or look it up (reads `dictionaries.db` in the library folder).

- Dictionaries: import Yomitan dictionary packages (zip, several at once, with progress and a stop button), enable /
  disable, drag to reorder, delete — the same `dictionaries.db` as on Windows.
- Anki mining: one click on a lookup result (Lyrics / Lapis note types, per-artist subdecks, the example's audio cut
  from that line), using the same mining code as the Windows app.

- Scraping: fill in album, year, genre, covers and artist photos from MusicBrainz / iTunes / Deezer (empty fields
  only); uncertain matches wait in a review queue for you to pick; single songs can be scraped too.
- Furigana (kanji only / whole word) and full-screen lyrics (blurred cover behind, the current line large and
  centred, click a line to jump to it).

Not there yet: Anki batch export and the mining report, replacing and editing lyrics. Architecture and build notes: [docs/clients.md](docs/clients.md)
(Chinese).

## First run

1. Open **Import**, pick a folder of audio you legally own, or choose individual files. An `.lrc` with the same
   name next to the audio is matched automatically.
2. Review the scan plan, then import.
3. If songs came in without lyrics, use **Settings → Library maintenance → Fill in missing lyrics**, or handle a
   single song from the library.
4. For covers, artist photos and band members, fill the gaps on the **Scrape** page.
5. For lookup, import Yomitan zips on the **Dictionaries** page (dictionaries registered by 0.1.x can be migrated in one click).
6. For mining, start Anki with AnkiConnect installed (see below).

### Data directory

`corpus.db`, `raw/audio`, `raw/lyrics_lrc`, `raw/covers` and `dictionaries.db` are looked up in this order:

1. the `JPOP_CORPUS_HOME` environment variable;
2. the directory picked in Settings (remembered in `%LOCALAPPDATA%\JPOP Corpus Tool\settings.json`);
3. walking up from the executable, the first database that **has its tables**;
4. the current working directory;
5. otherwise `%LOCALAPPDATA%\JPOP Corpus Tool`, **creating an empty library there**.

So a fresh install opens straight away with an empty library. If you already have one (coming from 0.1.x, say),
point **Settings → About → Library → Change directory…** at it and restart. The data directory lives outside the
install directory, so uninstalling never takes your songs and corpus with it.

The schema is versioned: an older library is brought up to date on open (tables and columns are only added,
existing data is untouched), and a library newer than the app is refused rather than damaged.
On macOS the default directory is `~/Library/Application Support/JPOP Corpus Tool`.

> **Furigana, clicking words in lyrics and tokenisation on import need the Sudachi dictionary.** It is not in the
> installer (207 MB unpacked). A banner offers it when it's missing, and **Settings → Lyrics → Tokeniser
> dictionary (Sudachi)** has a Download button — a 43 MB `.xz` from this repository's `sudachi-dict-core`
> release (interrupted downloads resume), checked against a fixed SHA-256 and unpacked into `sudachi/` in the
> library directory. It is fetched once per library directory. The same card can instead copy it from a folder
> you already have (another library directory, a 0.1.x project or its venv). Once it's installed, songs imported
> without tokenisation are tokenised on the spot.
>
> The dictionary is looked up in this order:
>
> 1. `venv/Lib/site-packages/sudachipy` + `sudachidict_core` in the library directory — the 0.1.x layout, still honoured;
> 2. `sudachi/` in the library directory — downloaded, copied in from Settings, or carried over by migration;
> 3. `resources/sudachi/` next to the executable — the installer doesn't ship one; only used if you put it there.
>
> Without it, playback, the Dictionaries page, search and mining still work; newly imported songs just aren't
> tokenised, so search and analysis can't see them.

### Anki

1. Install and start [Anki](https://apps.ankiweb.net/).
2. Install [AnkiConnect (add-on code 2055492159)](https://ankiweb.net/shared/info/2055492159).
3. Choose a deck and note type in **Settings → Mining** (the bundled **Lyrics** type is the default and is created on first use).

AnkiConnect listens on `http://127.0.0.1:8765` only, and this tool sends your corpus or Anki data nowhere.
Mining **only appends**: a word you already made a card for is skipped, never overwritten.

## Development

### Prerequisites

- [Node.js](https://nodejs.org/) 20+
- [Rust](https://rustup.rs/) stable, with the MSVC C++ build tools (SQLite, LAME and Rubber Band are compiled from source)
- Windows 10/11 with the WebView2 runtime
- For the macOS client: macOS 14+ and Xcode 15.3+ (Swift 5.10)

### Run

```powershell
git clone https://github.com/Yisoragoto/jpop-corpus-tool.git
cd jpop-corpus-tool\app
npm install
npm run app:dev
```

### Build

```powershell
npm run app:build    # installers land in rust/target/release/bundle/
```

### macOS client

```bash
./macos/scripts/build-rust.sh      # build the Rust core as libjp_ffi.a (pass "universal" for a fat binary)
cd macos && swift run JPCorpus      # run it; ./scripts/bundle.sh makes a .app
swift test                          # calls Rust through the real static library: data shapes, import and search end to end
```

### Checks

```powershell
cd app
npm run typecheck
npm test
cd ..\rust
cargo test --workspace
cargo clippy --workspace --all-targets
```

The first launch creates an empty library in the data directory. `examples/` shows the data formats without any
copyrighted content.

CI runs two workflows: `check.yml` (frontend typecheck and tests, Rust tests and clippy) and `macos.yml` on a macOS
runner (Rust static library, `swift build`, `swift test` — including "empty folder → import → tokenise → search"
with a cached Sudachi dictionary — and the .app bundle).

### Project layout

```text
app/           Frontend (React 19 + TypeScript strict + Vite)
rust/          Rust workspace
  jp-app/        Tauri shell: commands, state, path permissions
  jp-corpus/     Data layer, all SQL lives here; search, analysis, sub-corpora, line spans, provenance, migrations
  jp-tokenizer/  Tokenisation and UPOS mapping (Sudachi)
  jp-setup/      Locating, downloading, verifying and resuming the tokenizer dictionary (shared by both clients)
  jp-audio/      Playback: WSOLA stretching, key change, spectrum, single-line loop
  jp-dict/       Yomitan-style lookup and dictionary import
  jp-anki/       Card mining and the mining report
  jp-scraper/    Track identification, covers, artist photos, band members, online lyrics
  jp-import/     Scan, plan, write
  jp-normalize/  Title / artist normalisation
  jp-ffi/        C ABI (JSON in/out) for the native macOS client, see docs/clients.md
macos/         Native macOS client (SwiftUI + AVFoundation) on the same Rust core
docs/          Design notes and reconciliation records per subsystem
legacy/        The 0.1.x PyQt app, kept for reference: gui.py, dialogs/, scripts/, packaging/
scripts/       Release self-check (release-check.ps1)
assets/fonts/  Bundled OFL fonts and their licences
examples/      Data-format samples with no copyrighted content
raw/           Local audio and lyrics; contents are git-ignored
```

`docs/` (in Chinese) records how each subsystem works, why it was done that way, and which numbers were
reconciled against the old version during the port:

| Doc | About |
|---|---|
| [clients.md](docs/clients.md) | One Rust core, two clients; how data maps to audio; counting rules for analysis |
| [provenance.md](docs/provenance.md) | Lyric versions, validation, revision history, field provenance, reproducible analysis runs |
| [import.md](docs/import.md) | Scan, plan, write; lyric matching |
| [tokenizer.md](docs/tokenizer.md) · [token-corrections.md](docs/token-corrections.md) | Tokenisation, POS mapping, token corrections |
| [dictionary.md](docs/dictionary.md) | The Yomitan lookup port and its reconciliation |
| [anki.md](docs/anki.md) | Mining, the Lyrics note type, the mining report |
| [scraper.md](docs/scraper.md) · [scraper-rust.md](docs/scraper-rust.md) | Scraping sources, matching and confidence |
| [rust-core.md](docs/rust-core.md) · [tauri-shell.md](docs/tauri-shell.md) | What each Rust crate does; the Tauri shell |

## Libraries

| Name | Licence |
|---|---|
| [Tauri 2](https://tauri.app/) | Apache-2.0 / MIT |
| [React 19](https://react.dev/) | MIT |
| [rodio](https://github.com/RustAudio/rodio) · [rodio-wsola](https://crates.io/crates/rodio-wsola) | MIT / Apache-2.0 · Apache-2.0 |
| [Symphonia](https://github.com/pdeljanov/Symphonia) (decoding) | MPL-2.0 |
| [sudachi.rs](https://github.com/WorksApplications/sudachi.rs) | Apache-2.0 |
| [rusqlite](https://github.com/rusqlite/rusqlite) · SQLite | MIT · Public Domain |
| [lofty](https://github.com/Serial-ATA/lofty-rs) (audio tags) | MIT / Apache-2.0 |
| [ureq](https://github.com/algesten/ureq) | MIT / Apache-2.0 |
| [serde](https://serde.rs/) | MIT / Apache-2.0 |

## Attribution

| Name | What | Licence |
|---|---|---|
| [Yomitan](https://github.com/yomidevs/yomitan) | Dictionary lookup, deinflection, structured content, pitch display | GPL-3.0-or-later |
| [Lapis](https://github.com/donkuri/lapis) | Basis for the bundled **Lyrics** note type | GPL-3.0 |
| [hoshidicts](https://github.com/Manhhao/hoshidicts) | Dictionary storage design (no code copied) | GPL-3.0-or-later |
| [Klee One](https://fonts.google.com/specimen/Klee+One) · [LXGW WenKai](https://github.com/lxgw/LxgwWenKai) | Bundled fonts | SIL OFL 1.1 |
| [Rubber Band Library](https://breakfastquay.com/rubberband/) | Key changes. Source in [`third_party/rubberband/`](third_party/rubberband/), compiled into the app | GPL-2.0-or-later |
| [LAME](https://lame.sourceforge.io/) | MP3 encoding of the audio clips on Anki cards, compiled into the app | LGPL-2.0-or-later |

Per-file provenance is noted in the file headers; the full statement is in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Copyright boundaries

Please do not commit or redistribute:

- commercial song audio, or lyrics and LRC files you have no right to;
- a `corpus.db`, processed corpus or Anki deck containing real lyrics;
- third-party Yomitan dictionaries without redistribution permission;
- local settings, caches, backups and user paths.

This tool processes material you provide and have the right to use. Copyright and fair-use rules differ by
jurisdiction; satisfy yourself about the ones that apply to you.

## License

Source code: [GNU General Public License v3.0 or later](LICENSE) (GPL-3.0-or-later).
Copyright (C) 2026 Yisoragoto and JPOP Corpus contributors.
