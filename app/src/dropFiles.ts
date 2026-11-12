/**
 * 往窗口里拖文件。默认由外壳（App）接住，打开「曲库 › 添加音乐」去扫。
 * 自己要接拖放的组件（「曲库 › 来源」把拖进来的文件夹加成来源）挂着的时候调 `claimDrops()`，外壳就不管这一下。
 */

/** Rust 侧 `jp_import::scan` 的 `AUDIO_EXTS`：放得出声、会入库的。`audioExtensions.test.ts` 把两边钉在一起 */
export const PLAYABLE_AUDIO_EXTENSIONS = ["flac", "mp3", "wav", "m4a", "ogg", "aac", "aif", "aiff"];

/** Rust 侧的 `UNSUPPORTED_AUDIO`：认得出是音频、但放不了（没有解码器，或是音乐客户端的加密下载格式） */
export const UNSUPPORTED_AUDIO_EXTENSIONS = [
  "opus", "wma", "ape", "wv", "tak", "tta", "dsf", "dff", "mka", "webm",
  "ncm",
  "qmc0", "qmc2", "qmc3", "qmc4", "qmc6", "qmc8", "qmcflac", "qmcogg", "tkm", "mflac", "mflac0", "mgg", "mgg0", "mgg1", "bkcmp3", "bkcflac",
  "kgm", "kgma", "vpr",
  "kwm",
];

/** 两张表合起来。只用来给文件选择框当筛选、拖放时分文件和文件夹，
 *  真正算不算音频由后端判断——两边不一致时，后端会把选错的文件报在 `ignored` 里。
 *
 *  放不了的**故意留在这里**：选得到，导入计划里才能告诉用户「这种格式不支持」；
 *  筛掉的话文件在选择框里凭空消失，用户只会以为没扫到。 */
export const AUDIO_EXTENSIONS = [...PLAYABLE_AUDIO_EXTENSIONS, ...UNSUPPORTED_AUDIO_EXTENSIONS];

export function isAudioPath(path: string): boolean {
  const lower = path.toLowerCase();
  return AUDIO_EXTENSIONS.some((ext) => lower.endsWith(`.${ext}`));
}

/** 拖进来的是什么：一个文件夹就扫这个文件夹，只有音频就扫这几首，别的分不清 */
export type DropTarget = { kind: "folder"; path: string } | { kind: "files"; paths: string[] } | { kind: "mixed" };

/** 网页这边分不出是文件夹还是文件，按扩展名分：不是音频扩展名的当文件夹（「Vol.2」这种带点的文件夹名也对） */
export function classifyDrop(paths: string[]): DropTarget | null {
  if (paths.length === 0) return null;
  const audio = paths.filter(isAudioPath);
  const folders = paths.filter((p) => !isAudioPath(p));
  if (folders.length === 0) return { kind: "files", paths: audio };
  if (folders.length === 1 && audio.length === 0) return { kind: "folder", path: folders[0]! };
  return { kind: "mixed" };
}

let claims = 0;

/** 挂着的时候自己接拖放。返回释放函数（当 useEffect 的清理用） */
export function claimDrops(): () => void {
  claims += 1;
  let released = false;
  return () => {
    if (released) return;
    released = true;
    claims -= 1;
  };
}

export function dropsClaimed(): boolean {
  return claims > 0;
}
