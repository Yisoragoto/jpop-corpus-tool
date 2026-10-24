import { Vec2, Vec3 } from "gl-matrix";
//#region src/interfaces.d.ts
/**
 * 拥有一个 HTML 元素的接口
 *
 * 可以通过 `getElement` 获取这个类所对应的 HTML 元素实例
 */
interface HasElement {
  /** 获取这个类所对应的 HTML 元素实例 */
  getElement(): HTMLElement;
}
/**
 * 实现了这个接口的东西需要在使用完毕后
 *
 * 手动调用 `dispose` 函数来销毁清除占用资源
 *
 * 以免产生泄露
 */
interface Disposable {
  /**
   * 销毁实现了该接口的对象实例，释放占用的资源
   *
   * 一般情况下，调用本函数后就不可以再调用对象的任何函数了
   */
  dispose(): void;
}
/** 一个歌词单词 */
interface LyricWordBase {
  /** 单词的起始时间，单位为毫秒 */
  startTime: number;
  /** 单词的结束时间，单位为毫秒 */
  endTime: number;
  /** 单词内容 */
  word: string;
}
interface LyricWord extends LyricWordBase {
  /** 单词的音译内容 */
  romanWord?: string;
  /** 单词内容是否包含冒犯性的不雅用语 */
  obscene?: boolean;
  /** 单词的注音内容 */
  ruby?: LyricWordBase[];
  /** JPOP Corpus Tool patch: 这个词在原文里的起始字符下标（Unicode 码点），会写到词元素的 data-char-start 上 */
  charStart?: number;
}
/** 一行歌词，存储多个单词 */
interface LyricLine {
  /**
   * 该行的所有单词
   * 如果是 LyRiC 等只能表达一行歌词的格式，这里就只会有一个单词且通常其始末时间和本结构的 `startTime` 和 `endTime` 相同
   */
  words: LyricWord[];
  /** 该行的翻译歌词，将会显示在主歌词行的下方 */
  translatedLyric: string;
  /** 该行的音译歌词，将会显示在翻译歌词行的下方 */
  romanLyric: string;
  /** 句子的起始时间，单位为毫秒 */
  startTime: number;
  /** 句子的结束时间，单位为毫秒 */
  endTime: number;
  /** 该行是否为背景歌词行，当该行歌词的上一句非背景歌词被激活时，这行歌词将会显示出来，注意每个非背景歌词下方只能拥有一个背景歌词 */
  isBG: boolean;
  /** 该行是否为对唱歌词行（即歌词行靠右对齐） */
  isDuet: boolean;
}
/**
 * 优化歌词行的配置选项
 */
interface OptimizeLyricOptions {
  /**
   * 规范化歌词中的空格
   *
   * 将多个连续空格替换为一个空格
   * @default true
   */
  normalizeSpaces?: boolean;
  /**
   * 是否将行级时间戳强行设为字级时间戳
   * @default true
   */
  resetLineTimestamps?: boolean;
  /**
   * 该选项已不再生效，歌词优化时始终会把多行背景人声转换为
   * 单行背景人声 + 主歌词行的形式
   *
   * @deprecated 现有播放器架构已不再支持多个连续的背景人声行
   */
  convertExcessiveBackgroundLines?: boolean;
  /**
   * 是否同步主歌词与背景人声的时间
   * @default true
   */
  syncMainAndBackgroundLines?: boolean;
  /**
   * 清洗非刻意的重叠，以免不必要的多行高亮效果
   *
   * 重叠**达到** 500ms 时视为有意重叠并予以保留
   *
   * 重叠**不足** 500ms，且满足下列条件之一时视为无意重叠：
   * * 重叠不超过 100ms
   * * 重叠时长不超过下一行时长的 10%
   *
   * 并截断上一行歌词的结束时间为下一行歌词的开始时间
   * @default true
   */
  cleanUnintentionalOverlaps?: boolean;
  /**
   * 尝试让歌词提前最多 600ms 开始
   *
   * 与上一行存在重叠时尝试提前 400ms
   *
   * 若重叠时长不足 400ms，则提前重叠时长的 70%
   * @default true
   */
  tryAdvanceStartTime?: boolean;
}
//#endregion
//#region src/bg-render/base.d.ts
declare abstract class AbstractBaseRenderer implements Disposable, HasElement {
  /**
   * 修改背景的流动速度，数字越大越快，默认为 8
   * @param speed 背景的流动速度，默认为 8
   */
  abstract setFlowSpeed(speed: number): void;
  /**
   * 修改背景的渲染比例，默认是 0.5
   *
   * 一般情况下这个程度既没有明显瑕疵也不会特别吃性能
   * @param scale 背景的渲染比例
   */
  abstract setRenderScale(scale: number): void;
  /**
   * 是否启用静态模式，即图片在更换后就会保持静止状态并禁用更新，以节省性能
   * @param enable 是否启用静态模式
   */
  abstract setStaticMode(enable: boolean): void;
  /**
   * 修改背景动画帧率，默认是 30 FPS
   *
   * 如果设置成 0 则会停止动画
   * @param fps 目标帧率，默认 30 FPS
   */
  abstract setFPS(fps: number): void;
  /**
   * 暂停背景动画，画面即便是更新了图片也不会发生变化
   */
  abstract pause(): void;
  /**
   * 恢复播放背景动画
   */
  abstract resume(): void;
  /**
   * 设置背景专辑资源，纹理加载并设置完成后会返回
   * @param albumSource 专辑的资源链接，可以是图片或视频链接，抑或是任意 img/video 元素，如果提供字符串链接且为视频则需要指定第二个参数
   */
  abstract setAlbum(albumSource: string | HTMLImageElement | HTMLVideoElement, isVideo?: boolean): Promise<void>;
  /**
   * 设置低频的音量大小，范围在 80hz-120hz 之间为宜，取值范围在 [0.0-1.0] 之间
   *
   * 部分渲染器会根据音量大小调整背景效果（例如根据鼓点跳动）
   *
   * 如果无法获取到类似的数据，请传入 1.0 作为默认值，或不做任何处理（默认值即 1.0）
   * @param volume 低频的音量大小，范围在 50hz-120hz 之间为宜，取值范围在 [0.0-1.0] 之间
   */
  abstract setLowFreqVolume(volume: number): void;
  /**
   * 设置背景是否根据“是否有歌词”这个特征调整自身效果，例如有歌词时会变得更加活跃
   *
   * 部分渲染器会根据这个特征调整自身效果
   *
   * 如果不确定是否需要赋值或无法知晓是否包含歌词，请传入 true 或不做任何处理（默认值为 true）
   *
   * @param hasLyric 是否有歌词，如不确定是否需要赋值，请传入 true 或不做任何处理（默认值为 true）
   */
  abstract setHasLyric(hasLyric: boolean): void;
  abstract dispose(): void;
  abstract getElement(): HTMLElement;
}
declare abstract class BaseRenderer extends AbstractBaseRenderer {
  protected canvas: HTMLCanvasElement;
  private observer;
  protected flowSpeed: number;
  protected currerntRenderScale: number;
  constructor(canvas: HTMLCanvasElement);
  setRenderScale(scale: number): void;
  /**
   * 当画板元素大小发生变化时此函数会被调用
   * 可以在此处重设和渲染器相关的尺寸设置
   * 考虑到初始化的时候元素不一定在文档中或出于某些特殊样式状态，尺寸长宽有可能会为 0，请注意进行特判处理
   * @param width 画板元素实际的物理像素宽度，有可能为 0
   * @param height 画板元素实际的物理像素高度，有可能为 0
   */
  protected onResize(width: number, height: number): void;
  /**
   * 修改背景的流动速度，数字越大越快，默认为 1
   * @param speed 背景的流动速度，默认为 1
   */
  setFlowSpeed(speed: number): void;
  /**
   * 是否启用静态模式，即图片在更换后就会保持静止状态并禁用更新，以节省性能
   * @param enable 是否启用静态模式
   */
  abstract override setStaticMode(enable: boolean): void;
  /**
   * 修改背景动画帧率，默认是 30 FPS
   *
   * 如果设置成 0 则会停止动画
   * @param fps 目标帧率，默认 30 FPS
   */
  abstract override setFPS(fps: number): void;
  /**
   * 暂停背景动画，画面即便是更新了图片也不会发生变化
   */
  abstract override pause(): void;
  /**
   * 恢复播放背景动画
   */
  abstract override resume(): void;
  /**
   * 设置背景专辑资源，纹理加载并设置完成后会返回
   * @param albumSource 专辑的资源链接，可以是图片或视频链接，抑或是任意 img/video 元素，如果提供字符串链接且为视频则需要指定第二个参数
   */
  abstract override setAlbum(albumSource: string | HTMLImageElement | HTMLVideoElement, isVideo?: boolean): Promise<void>;
  /** 停止监听画板尺寸，供构造失败等尚未接管画板所有权的路径清理 */
  protected disconnectResizeObserver(): void;
  dispose(): void;
  override getElement(): HTMLElement;
}
//#endregion
//#region src/bg-render/gl-program.d.ts
/**
 * 着色器程序可用的渲染上下文。
 *
 * 目前的背景渲染器以 WebGL1 为主，但这个封装本身不依赖特定版本，所以两者
 * 都接受。
 */
type GLRenderingContext = WebGLRenderingContext | WebGL2RenderingContext;
/**
 * 对 WebGL 着色器程序的一层薄封装，负责编译、链接、缓存 uniform 位置。
 */
declare class GLProgram implements Disposable {
  private readonly label;
  private gl;
  program: WebGLProgram;
  private vertexShader;
  private fragmentShader;
  readonly attrs: {
    [name: string]: number;
  };
  private uniformLocations;
  constructor(gl: GLRenderingContext, vertexShaderSource: string, fragmentShaderSource: string, label?: string);
  private createShader;
  private createProgram;
  use(): void;
  private notFoundUniforms;
  private warnUniformNotFound;
  /**
   * 取 uniform 位置并缓存。逐帧设置几十个 uniform 时，省下的
   * `getUniformLocation` 调用相当可观。
   */
  private getUniformLocation;
  setUniform1f(name: string, value: number): void;
  setUniform2f(name: string, value1: number, value2: number): void;
  setUniform3f(name: string, value1: number, value2: number, value3: number): void;
  setUniform4f(name: string, value1: number, value2: number, value3: number, value4: number): void;
  setUniform1i(name: string, value: number): void;
  setUniform1fv(name: string, value: Float32Array): void;
  setUniform3fv(name: string, value: Float32Array): void;
  dispose(): void;
}
//#endregion
//#region src/bg-render/palette/types.d.ts
/**
 * @fileoverview
 * 调色板取色器的公共类型。
 *
 * 移植自 Storyteller-Studios/Impressionist（MIT）：
 * - `Impressionist/Abstractions/HSVColor.cs`
 * - `Impressionist/Abstractions/PaletteResult.cs`
 *
 * 颜色一律用 `[x, y, z]` 三元组表示。在 RGB 空间时分量范围是 [0, 255]，
 * 与 C# 原实现的 `Vector3` 约定保持一致；在 LAB 空间时则是 L/a/b 的原始取值。
 *
 * @see https://github.com/Storyteller-Studios/Impressionist/blob/master/Impressionist/Abstractions/HSVColor.cs
 * @see https://github.com/Storyteller-Studios/Impressionist/blob/master/Impressionist/Abstractions/PaletteResult.cs
 */
/** 一个颜色向量，语义随所处色彩空间而定。 */
type ColorVec3 = [number, number, number];
/** HSV 颜色，H 取值 [0, 360)，S/V 取值 [0, 100]。 */
interface HSVColor {
  h: number;
  s: number;
  v: number;
}
/** 直方图中的一项：一个颜色及其出现次数。 */
interface ColorCount {
  color: ColorVec3;
  count: number;
}
/** 主题色结果。 */
interface ThemeColorResult {
  /** RGB 空间下的主题色，分量范围 [0, 255]。 */
  color: ColorVec3;
  /** 该主题色是否偏暗（L* <= 50）。 */
  colorIsDark: boolean;
}
/** 调色板结果。 */
interface PaletteResult {
  /** 调色板颜色，RGB 空间，分量范围 [0, 255]，长度恒等于请求的数量。 */
  palette: ColorVec3[];
  /** 该调色板整体是否偏暗。 */
  paletteIsDark: boolean;
  /** 生成调色板时顺带算出的主题色。 */
  themeColor: ThemeColorResult;
}
/**
 * 调色板的用途，决定取色时的取舍。
 *
 * - `"accent"`：Impressionist 的取向，取强调色用。只保留与主题色明暗一致的候选
 *   色，整套调色板压在明暗轴的一侧，作为强调色时才和界面拉得开对比；亮色封面还
 *   会偏好更聚拢的一支，免得强调色散得没有章法。
 * - `"dominant"`：要封面本身的主色，铺满全屏的背景用。不按明暗筛候选色（否则
 *   L* 落在 (40, 60) 的中间调会被整段丢掉，封面的标志色若在这一带就整个消失），
 *   择优时一律取更分散的一支 —— 背景要的是封面的颜色跨度，不是和界面的对比度。
 */
type PaletteIntent = "accent" | "dominant";
//#endregion
//#region src/bg-render/palette/auto.d.ts
/**
 * 同时跑两种取色算法并择优：暗色调色板偏好更分散的结果，亮色调色板反之偏好
 * 更聚拢的结果（八叉树在亮色上容易退化成一片惨白，所以 0 分散度直接判负）。
 *
 * 但分散度只在两者给出同样多的颜色时才有可比性。调用方要几个色就是几个色，被
 * 重复填充凑满的结果对任何按数量取色的用法都是退化的 —— 拿去做多点渐变会直接
 * 塌成两色平铺 —— 而重复恰恰会拉低分散度，于是在亮色分支里被当成「更聚拢」选
 * 中。所以先比互不相同的颜色数，同数时才轮到原本的分散度规则。
 *
 * 「亮色偏好更聚拢」同样是强调色的取向：强调色散得没有章法不好用，但背景要的正
 * 是封面的颜色跨度，更分散才更像那张封面。所以 `intent` 为 `"dominant"` 时一律
 * 取更分散的一支，不再分明暗两套。见 {@link PaletteIntent}。
 */
declare function createAutoPalette(sourceColors: readonly ColorCount[], clusterCount: number, ignoreWhite?: boolean, toLab?: boolean, useKMeansPP?: boolean, intent?: PaletteIntent): PaletteResult;
//#endregion
//#region src/bg-render/palette/color-utilities.d.ts
declare function rgbToHsv(color: ColorVec3): HSVColor;
declare function hsvToRgb(hsv: HSVColor): ColorVec3;
declare function rgbToXyz(rgb: ColorVec3): ColorVec3;
declare function xyzToRgb(xyz: ColorVec3): ColorVec3;
declare function xyzToLab(xyz: ColorVec3): ColorVec3;
declare function labToXyz(lab: ColorVec3): ColorVec3;
declare function rgbToLab(rgb: ColorVec3): ColorVec3;
declare function labToRgb(lab: ColorVec3): ColorVec3;
declare function channelToLinear(value: number): number;
/**
 * 把 sRGB 颜色转换到 OkLab。
 *
 * 与本文件其它函数不同，入参与返回的分量取值均为 [0, 1]：OkLab 的矩阵本就定义
 * 在归一化的线性 sRGB 上，没必要为了统一风格多做一次 255 的往返。
 *
 * OkLab 是感知均匀的，在它里面对两个颜色取中点不会像 sRGB 那样发暗发浊，所以
 * 需要混色或者做颜色过渡的地方应当先转到这里来。
 *
 * @see https://bottosson.github.io/posts/oklab/
 */
declare function srgbToOkLab(rgb: ColorVec3): ColorVec3;
declare function yToLStar(y: number): number;
/** 调色板取色时判定「暗色候选」的阈值，比主题色判定更严格。 */
declare function paletteRgbLStarIsDark(rgb: ColorVec3): boolean;
/** 调色板取色时判定「亮色候选」的阈值。 */
declare function paletteRgbLStarIsLight(rgb: ColorVec3): boolean;
/** 主题色的明暗判定。 */
declare function rgbLStarIsDark(rgb: ColorVec3): boolean;
//#endregion
//#region src/bg-render/palette/histogram.d.ts
/** 可以用来取色的图像资源。 */
type HistogramSource = HTMLImageElement | HTMLVideoElement | ImageBitmap | ImageData;
/**
 * 构建颜色直方图。全透明像素会被跳过，半透明像素按原色计入。
 *
 * @param source 图像资源
 * @param sampleSize 统计前缩放到的最长边像素数，默认 64
 */
declare function buildColorHistogram(source: HistogramSource, sampleSize?: number): ColorCount[];
//#endregion
//#region src/bg-render/palette/kmeans.d.ts
/** 一个确定性的伪随机数发生器，返回 [0, 1) 区间的浮点数。 */
type RandomSource = () => number;
/** 计算主题色，对应 C# 的 `KMeansPaletteGenerator.CreateThemeColor`。 */
declare function createThemeColor(sourceColors: readonly ColorCount[], ignoreWhite?: boolean, toLab?: boolean, random?: RandomSource): ThemeColorResult;
/**
 * 生成调色板，对应 C# 的 `KMeansPaletteGenerator.CreatePalette`。
 *
 * `intent` 见 {@link PaletteIntent}：默认的 `"accent"` 沿用 Impressionist 的行为，
 * 铺满全屏的背景应当传 `"dominant"`。
 */
declare function createKMeansPalette(sourceColors: readonly ColorCount[], clusterCount: number, themeColor: ThemeColorResult, ignoreWhite?: boolean, toLab?: boolean, useKMeansPP?: boolean, intent?: PaletteIntent, random?: RandomSource): PaletteResult;
//#endregion
//#region src/bg-render/palette/octtree.d.ts
/**
 * 生成调色板，对应 C# 的 `OctTreePaletteGenerator.CreatePalette`。
 *
 * `intent` 见 {@link PaletteIntent}：默认的 `"accent"` 沿用 Impressionist 的行为，
 * 铺满全屏的背景应当传 `"dominant"`。
 */
declare function createOctTreePalette(sourceColors: readonly ColorCount[], clusterCount: number, themeColor?: ThemeColorResult, ignoreWhite?: boolean, intent?: PaletteIntent): PaletteResult;
//#endregion
//#region src/bg-render/palette/index.d.ts
/** 取色算法。 */
type PaletteAlgorithm = "auto" | "kmeans" | "octtree";
/** {@link createPaletteFromImage} 的可选项。 */
interface CreatePaletteOptions {
  /** 取色算法，默认 `"auto"`。 */
  algorithm?: PaletteAlgorithm;
  /** 是否忽略接近纯白的颜色，默认 `false`。 */
  ignoreWhite?: boolean;
  /** K-Means 是否在 LAB 空间聚类，默认 `false`。 */
  toLab?: boolean;
  /** K-Means 是否使用 K-Means++ 初始化，默认 `false`。 */
  useKMeansPP?: boolean;
  /** 调色板的用途，默认 `"accent"`。见 {@link PaletteIntent}。 */
  intent?: PaletteIntent;
  /** 统计直方图前缩放到的最长边像素数，默认 64。 */
  sampleSize?: number;
}
/**
 * 从图像资源提取指定数量的主色。
 *
 * 整个流程是同步的：封面会先被缩到 64×64 再统计，因此在主线程上通常只需要几
 * 毫秒，不值得为它单独开一个 Worker。
 *
 * @param source 图像资源，可以是 img/video 元素、`ImageBitmap` 或 `ImageData`
 * @param clusterCount 需要的颜色数量，返回的调色板长度恒等于该值
 */
declare function createPaletteFromImage(source: HistogramSource, clusterCount: number, options?: CreatePaletteOptions): PaletteResult;
//#endregion
//#region src/bg-render/isolation/index.d.ts
/** Isolation 渲染器的可调选项。 */
interface IsolationRendererOptions {
  /** 是否启用 lightwave 明度调制，默认 `false`。 */
  lightWave: boolean;
  /** 是否启用屏幕空间抖动，默认 `true`。 */
  dithering: boolean;
  /** 取色算法，默认 `"auto"`。 */
  paletteAlgorithm: PaletteAlgorithm;
}
declare class IsolationRenderer extends BaseRenderer {
  /**
   * 新建实例时采用的默认选项。
   *
   * 该对象也可作为配置界面的初始值；实例创建后的调整统一走
   * {@link setOptions}。
   */
  static readonly defaultOptions: Readonly<IsolationRendererOptions>;
  /** 当前环境是否支持该渲染器，选择渲染器前应先问一句。 */
  static isSupported(): boolean;
  private gl;
  private program;
  private quadBuffer;
  private contextLost;
  private _disposed;
  private options;
  private tickHandle;
  private lastTickTime;
  private lastFrameTime;
  private frameTime;
  private maxFPS;
  private paused;
  private staticMode;
  private targetWidth;
  private targetHeight;
  private currentWidth;
  private currentHeight;
  private albumRequestId;
  private albumSource?;
  /**
   * 调色板过渡已经过的毫秒数。
   *
   * 这里刻意不用 `performance.now()`：过渡必须和渲染时钟走同一套时间，否则
   * 暂停、静态模式或限帧的时候过渡进度会和画面对不上。
   */
  private paletteTransitionElapsed;
  /** 过渡起点、终点与当前帧的颜色，均为 OkLab，四个颜色首尾相接。 */
  private readonly fromColors;
  private readonly toColors;
  private readonly colorBuffer;
  /** 取色结果的暂存区，避免每次换封面都新建数组。 */
  private readonly nextColors;
  private readonly randomValues;
  /**
   * 渐变流动参数，依次是波纹频率、波纹幅度、流动速度（已含方向）与渐变轴倾角
   * （弧度）。原实现是在片元着色器里用随机哈希现算的，但它们对整个 draw call
   * 都是常量，挪到 CPU 上由 {@link rollRandomParameters} 随机一次即可 —— 这里
   * 含随机量，若真的逐帧重算，画面会逐帧剧烈跳变。
   */
  private readonly flowParams;
  /** 渐变轴叠加在噪声角度上的抖动，单位弧度，同样是整帧常量。 */
  private angleJitter;
  private readonly paletteOrder;
  constructor(canvas: HTMLCanvasElement);
  private initializeGLResources;
  private readonly onContextLost;
  private readonly onContextRestored;
  /** 重掷整张封面期间保持不变的随机参数，避免画面逐帧跳变。 */
  private rollRandomParameters;
  /** 调整渲染器选项，会立即生效。 */
  setOptions(patch: Partial<IsolationRendererOptions>): void;
  private updatePaletteFromSource;
  private transitionToColors;
  /** 按当前过渡进度就地更新 {@link colorBuffer}，不产生任何中间数组。 */
  private updateColorBuffer;
  private checkIfResize;
  private onRedraw;
  private onTick;
  private readonly onTickBinded;
  private requestTick;
  protected override onResize(width: number, height: number): void;
  override setStaticMode(enable: boolean): void;
  override setFPS(fps: number): void;
  override pause(): void;
  override resume(): void;
  private resetFrameClock;
  /**
   * 该次 `setAlbum` 是否还是最新的一次。
   *
   * 与 `MeshGradientRenderer` 不同，这里不看 `contextLost`：取色全在 CPU
   * 上做，上下文丢了也照样能把调色板算完存着，等上下文恢复直接就能画。
   */
  private isCurrentAlbumRequest;
  override setAlbum(albumSource?: string | HTMLImageElement | HTMLVideoElement, isVideo?: boolean): Promise<void>;
  override setLowFreqVolume(_volume: number): void;
  override setHasLyric(_hasLyric: boolean): void;
  override dispose(): void;
}
//#endregion
//#region src/bg-render/mesh-renderer/index.d.ts
declare class ControlPoint {
  color: Vec3;
  location: Vec2;
  uTangent: Vec2;
  vTangent: Vec2;
  private _uRot;
  private _vRot;
  private _uScale;
  private _vScale;
  constructor();
  get uRot(): number;
  get vRot(): number;
  set uRot(value: number);
  set vRot(value: number);
  get uScale(): number;
  get vScale(): number;
  set uScale(value: number);
  set vScale(value: number);
  private updateUTangent;
  private updateVTangent;
}
declare class MeshGradientRenderer extends BaseRenderer {
  /**
   * 当前环境是否支持此渲染器
   */
  static isSupported(): boolean;
  private gl;
  private contextLost;
  private albumRequestId;
  private albumLoadController?;
  private lastImageData?;
  private lastFrameTime;
  private frameTime;
  private lastTickTime;
  private smoothedVolume;
  private volume;
  private tickHandle;
  private maxFPS;
  private paused;
  private staticMode;
  private mainProgram;
  private quadProgram;
  private quadBuffer;
  private fbo;
  private fboTexture;
  private manualControl;
  private reduceImageSizeCanvas;
  private targetSize;
  private currentSize;
  private isNoCover;
  private meshStates;
  private _disposed;
  private frameCount;
  private lastFPSUpdate;
  private currentFPS;
  private enablePerformanceMonitoring;
  private isCurrentAlbumRequest;
  private initializeGLResources;
  private createMeshState;
  private onContextLost;
  private onContextRestored;
  setManualControl(enable: boolean): void;
  setWireFrame(enable: boolean): void;
  getControlPoint(x: number, y: number): ControlPoint | undefined;
  resizeControlPoints(width: number, height: number): void;
  resetSubdivition(subDivisions: number): void;
  private onTick;
  private checkIfResize;
  private updateFBO;
  private onRedraw;
  private onTickBinded;
  private requestTick;
  constructor(canvas: HTMLCanvasElement);
  protected override onResize(width: number, height: number): void;
  override setStaticMode(enable: boolean): void;
  override setFPS(fps: number): void;
  override pause(): void;
  override resume(): void;
  override setAlbum(albumSource?: string | HTMLImageElement | HTMLVideoElement, isVideo?: boolean): Promise<void>;
  override setLowFreqVolume(volume: number): void;
  override setHasLyric(_hasLyric: boolean): void;
  override dispose(): void;
  enablePerformanceMonitor(enable: boolean): void;
  getCurrentFPS(): number;
  private updatePerformanceStats;
}
//#endregion
//#region src/bg-render/pixi-renderer.d.ts
declare class PixiRenderer extends BaseRenderer {
  protected override canvas: HTMLCanvasElement;
  /**
   * 当前环境是否支持此渲染器
   */
  static isSupported(): boolean;
  private app;
  private curContainer?;
  private staticMode;
  private lastContainer;
  private onTick;
  constructor(canvas: HTMLCanvasElement);
  protected override onResize(width: number, height: number): void;
  override setRenderScale(scale: number): void;
  private rebuildFilters;
  override setStaticMode(enable?: boolean): void;
  override setFPS(fps: number): void;
  override pause(): void;
  override resume(): void;
  override setLowFreqVolume(_volume: number): void;
  override setHasLyric(_hasLyric: boolean): void;
  override setAlbum(albumSource?: string | HTMLImageElement | HTMLVideoElement, isVideo?: boolean): Promise<void>;
  override dispose(): void;
  override getElement(): HTMLElement;
}
//#endregion
//#region src/bg-render/index.d.ts
declare class BackgroundRender<Renderer extends BaseRenderer> implements AbstractBaseRenderer {
  private element;
  private renderer;
  constructor(renderer: Renderer, canvas: HTMLCanvasElement);
  /**
   * 获取被包装的渲染器实例。
   *
   * 各个渲染器有自己特有的可调项（例如 {@link IsolationRenderer.setOptions}），
   * 这些项没法通过统一的 `AbstractBaseRenderer` 接口下发，需要拿到实例本体。
   */
  getRenderer(): Renderer;
  static new<Renderer extends BaseRenderer>(type: {
    new (canvas: HTMLCanvasElement): Renderer;
  }): BackgroundRender<Renderer>;
  setRenderScale(scale: number): void;
  setFlowSpeed(speed: number): void;
  setStaticMode(enable: boolean): void;
  setFPS(fps: number): void;
  pause(): void;
  resume(): void;
  setLowFreqVolume(volume: number): void;
  setHasLyric(hasLyric: boolean): void;
  setAlbum(albumSource: string | HTMLImageElement | HTMLVideoElement, isVideo?: boolean): Promise<void>;
  getElement(): HTMLCanvasElement;
  dispose(): void;
}
//#endregion
//#region src/utils/time.d.ts
declare const DURATION_BRAND: unique symbol;
declare const MEDIA_TIME_BRAND: unique symbol;
/**
 * 一段时长，内部以毫秒浮点数表示
 */
type Duration = {
  readonly [DURATION_BRAND]: true;
};
/**
 * 媒体时间轴上的一个时间点，内部以毫秒浮点数表示
 *
 * 原点为歌曲起始 0ms
 */
type MediaTime = {
  readonly [MEDIA_TIME_BRAND]: true;
};
declare const Duration: {
  readonly ZERO: Duration;
  readonly fromMillis: (ms: number) => Duration;
  readonly fromSecs: (s: number) => Duration;
  readonly asMillis: (d: Duration) => number;
  readonly asSecsF64: (d: Duration) => number;
  readonly add: (a: Duration, b: Duration) => Duration;
  readonly sub: (a: Duration, b: Duration) => Duration;
  readonly saturatingSub: (a: Duration, b: Duration) => Duration;
  readonly mulF64: (d: Duration, factor: number) => Duration;
  readonly divDuration: (a: Duration, b: Duration) => number;
  readonly min: (a: Duration, b: Duration) => Duration;
  readonly max: (a: Duration, b: Duration) => Duration;
  readonly clampPositive: (d: Duration) => Duration;
  readonly isZero: (d: Duration) => boolean;
  readonly isFinite: (d: Duration) => boolean;
};
declare const MediaTime: {
  readonly ZERO: MediaTime;
  readonly fromMillis: (ms: number) => MediaTime;
  readonly asMillis: (t: MediaTime) => number;
  readonly since: (a: MediaTime, b: MediaTime) => Duration;
  readonly saturatingSince: (a: MediaTime, b: MediaTime) => Duration;
  readonly add: (t: MediaTime, d: Duration) => MediaTime;
  readonly sub: (t: MediaTime, d: Duration) => MediaTime;
  readonly min: (a: MediaTime, b: MediaTime) => MediaTime;
  readonly max: (a: MediaTime, b: MediaTime) => MediaTime;
  readonly cmp: (a: MediaTime, b: MediaTime) => number;
  readonly round: (t: MediaTime) => MediaTime;
};
declare namespace spring_d_exports {
  export { Spring, SpringParams };
}
/** MIT License github.com/pushkine/ */
interface SpringParams {
  mass: number;
  damping: number;
  stiffness: number;
  soft: boolean;
}
declare class Spring {
  private currentPosition;
  private targetPosition;
  private currentTime;
  private params;
  private currentSolver;
  private getV;
  private getV2;
  private queueParams;
  private queuePosition;
  constructor(currentPosition?: number);
  private resetSolver;
  arrived(): boolean;
  setPosition(targetPosition: number): void;
  update(delta?: Duration): void;
  updateParams(params: Partial<SpringParams>, delay?: Duration): void;
  setTargetPosition(targetPosition: number, delay?: Duration): void;
  getCurrentPosition(): number;
}
//#endregion
//#region src/lyric-player/base/bottom-line.d.ts
/** 底栏的位移动画弹簧 */
interface BottomLineTransforms {
  posY: Spring;
}
/**
 * 底栏组件的抽象接口
 */
interface BottomLine extends HasElement, Disposable {
  /**
   * 将底栏放回重建歌词视图时的初始位置
   */
  resetPosition(): void;
  /**
   * 获取供外部插入内容的元素
   */
  getContentElement?(): HTMLElement;
  /**
   * 底栏当前测量得到的尺寸
   *
   * 由播放器的 ResizeObserver 回调写入
   */
  lineSize: [number, number];
  /**
   * 底栏的位移弹簧，目标位置与参数由播放器驱动
   */
  readonly lineTransforms: BottomLineTransforms;
  /**
   * 设置底栏是否处于聚焦状态
   *
   * 一般在歌曲播放完毕且底栏有内容时聚焦到底栏并设为 true
   */
  setFocused(focused: boolean): void;
  /**
   * 设置底栏的目标位置与模糊值
   * @param top 底栏的 Y 坐标
   * @param blur 底栏的模糊度
   * @param immediate 为 true 时绕过弹簧立刻跳转至目标位置
   * @param delay 弹簧过渡的延迟
   */
  setTransform(top?: number, blur?: number, immediate?: boolean, delay?: Duration): void;
  /**
   * 逐帧推进弹簧动画并应用样式
   * @param delta 距离上一次调用的时长
   */
  update(delta?: Duration): void;
}
//#endregion
//#region src/lyric-player/base/consts.d.ts
type ValueOf<T extends Record<PropertyKey, unknown>> = T[keyof T];
/** 歌词中不雅用语的掩码模式 */
declare const MaskObsceneWordsMode: {
  /** 禁用任何不雅用语掩码 */
  readonly Disabled: "";
  /** 完全掩码所有不雅用语 */
  readonly FullMask: "full-mask";
  /** 保留首尾字符，屏蔽中间字符 */
  readonly PartialMask: "partial-mask";
};
/** 歌词中不雅用语的掩码模式枚举类型，见 {@link MaskObsceneWordsMode} */
type MaskObsceneWordsMode = ValueOf<typeof MaskObsceneWordsMode>;
/**
 * 歌词行的渲染模式
 * @internal
 */
declare const LyricLineRenderMode: {
  readonly SOLID: 0;
  readonly GRADIENT: 1;
};
/**
 * 歌词行的渲染模式枚举类型，见 {@link LyricLineRenderMode}
 * @internal
 */
type LyricLineRenderMode = ValueOf<typeof LyricLineRenderMode>;
/** 布局对齐锚点 */
declare const LayoutAlignAnchor: {
  readonly Top: "top";
  readonly Center: "center";
  readonly Bottom: "bottom";
};
/** 布局对齐锚点枚举类型，见 {@link LayoutAlignAnchor} */
type LayoutAlignAnchor = ValueOf<typeof LayoutAlignAnchor>;
/**
 * 触发排版布局更新的原因场景
 */
declare const LayoutReason: {
  /** 正常播放时间推进 */
  readonly PlaybackTick: "playback-tick";
  /** 容器或窗口尺寸调整 */
  readonly Resize: "resize";
  /** 用户交互挂起开始（触摸/滚轮触发） */
  readonly InteractionStart: "interaction-start";
  /** 连续高频滚动（手指触摸滑动或松手后的 RAF 惯性滑动） */
  readonly ContinuousScroll: "continuous-scroll";
  /** 离散单步滚动（鼠标滚轮单次滚动） */
  readonly DiscreteScroll: "discrete-scroll";
  /** 跳转播放进度 */
  readonly Seek: "seek";
  /** 重新构建歌词视图 */
  readonly RebuildView: "rebuild-view";
  /** 视图结构或样式配置改变 */
  readonly ConfigChange: "config-change";
};
/** 触发排版布局更新的原因场景枚举类型，见 {@link LayoutReason} */
type LayoutReason = ValueOf<typeof LayoutReason>;
/**
 * 对应各个 LayoutReason 的排版执行策略定义
 */
interface LayoutStrategy {
  /** 是否禁用阶梯交错动画 */
  disableStagger: boolean;
  /** 是否重置间奏圆点动画 */
  resetInterlude: boolean;
  /**
   * 是否瞬移 Y 轴位置而不经过弹簧动画
   *
   * 一般用于触摸拖动中，避免弹簧动画导致拖动不跟手
   */
  snapPosY: boolean;
}
/**
 * 排版原因到排版执行策略的映射字典
 */
declare const LayoutReasonStrategyMap: Record<LayoutReason, LayoutStrategy>;
/**
 * 单帧动画时长的钳制上限，用于页面挂起等场景
 *
 * 此时首帧 update 的 delta 携带整个挂起时长，直接透传会导致动画以数秒的时长过冲
 */
declare const MAX_FRAME_DELTA: Duration;
//#endregion
//#region src/lyric-player/base/line.d.ts
interface LineTransforms {
  scale: Spring;
}
/**
 * 所有标准歌词行的基类
 * @internal
 */
declare abstract class LyricLineBase extends EventTarget implements Disposable {
  protected top: number;
  protected scale: number;
  protected blur: number;
  protected opacity: number;
  protected delay: Duration;
  protected isUiDirty: boolean;
  readonly lineTransforms: LineTransforms;
  /**
   * 用于 CJK 词语边界检测的分词器
   */
  static readonly wordSegmenter: Intl.Segmenter | null;
  /**
   * Unicode 标准的全局 Grapheme Cluster 分词器
   * 用于正确处理 emoji、复合字符等
   */
  static readonly graphemeSegmenter: Intl.Segmenter | null;
  abstract getLine(): LyricLine;
  abstract enable(time?: number, shouldPlay?: boolean): void;
  abstract disable(): void;
  abstract resume(): void;
  abstract pause(): void;
  abstract onLineSizeChange(size: [number, number]): void;
  abstract commitChanges(): void;
  setTransform(scale?: number, opacity?: number, blur?: number, delay?: Duration, _mode?: LyricLineRenderMode): void;
  rebuildElement(): void;
  /**
   * 判定歌词是否可以应用强调辉光效果
   *
   * 果子在对辉光效果的解释是一种强调（emphasized）效果
   *
   * 条件是一个单词时长大于等于 1s 且长度小于等于 7
   *
   * @param word 单词
   * @returns 是否可以应用强调辉光效果
   */
  static shouldEmphasize(word: LyricWord): boolean;
  abstract update(delta?: Duration): void;
  dispose(): void;
}
//#endregion
//#region src/lyric-player/base/group.d.ts
interface LyricPlayerFlags {
  getEnableSpring(): boolean;
  getEnableScale(): boolean;
  getIsPlaying(): boolean;
  getAlwaysPostpositionBackground(): boolean;
}
declare abstract class LyricLineGroupBase<T extends LyricLineBase = LyricLineBase> implements Disposable {
  mainLine: T;
  bgLine?: T | undefined;
  protected abstract readonly lyricPlayer: LyricPlayerFlags;
  posY: Spring;
  bgSlideY: Spring;
  top: number;
  delay: Duration;
  isActive: boolean;
  opacity: number;
  blur: number;
  isBgFirst: boolean;
  protected isUiDirty: boolean;
  constructor(mainLine: T, bgLine?: T | undefined);
  get startTime(): MediaTime;
  get endTime(): MediaTime;
  onLineSizeChange(size: [number, number]): void;
  onBgSizeChange?(size: [number, number]): void;
  abstract getElement(): Element;
  setTransform(top: number, immediate: boolean, delay: Duration, isActive: boolean, opacity: number, blur: number): void;
  private setLineTransformations;
  protected abstract renderStyles(): void;
  /**
   * 根据当前动画位置判断歌词行是否处于渲染范围内
   *
   * @param includeOverscan 是否包含 overscan 渲染缓冲范围，默认包含；
   * 传入 false 时，仅判断歌词行是否在真实视口范围内
   */
  abstract isInRenderRange(includeOverscan?: boolean): boolean;
  update(delta?: Duration): void;
  commitChanges(): void;
  rebuildAllLines(): void;
  enable(time?: number, shouldPlay?: boolean): void;
  disable(): void;
  dispose(): void;
}
//#endregion
//#region src/lyric-player/base/interlude-dots.d.ts
/**
 * 间奏点在特定时刻的渲染快照
 */
interface InterludeDotsSnapshot {
  /**
   * 动画是否仍处于活跃状态
   */
  readonly isActive: boolean;
  /**
   * 三颗圆点各自的不透明度
   */
  readonly dotOpacities: readonly [number, number, number];
  /**
   * 容器缩放值
   */
  readonly scale: number;
  /**
   * 容器整体不透明度
   */
  readonly opacity: number;
}
declare abstract class InterludeDotsBase implements Disposable {
  private left;
  private top;
  readonly posY: Spring;
  /**
   * 下一次设置变换位置时是否直接吸附到目标位置
   *
   * 演出重建或位置跳变时新旧坐标可能相距很远，走弹簧会看到间奏点从旧位置滑入的残影
   */
  private shouldSnapPosY;
  private currentTime;
  private playing;
  private phase;
  private mode;
  private fadeElapsedMs;
  private fadeInitialOpacity;
  private startTime;
  private endTime;
  private anchorTime;
  private delayEndMs;
  private bodyEndMs;
  private totalEndMs;
  private breathePeriodMs;
  private segmentMs;
  private dot3DurationMs;
  private dot3Target;
  private readonly mutDotOpacities;
  private readonly snapshot;
  /**
   * 设置间奏区间并锚定演出时间
   *
   * @param interlude 间奏起止时间
   * @param currentTime 当前播放时间，用于把演出重锚到该时刻；未传入时使用间奏起点
   * @param forceReset 是否强制重建演出，如跳转播放进度或重建歌词视图时
   * @param anchorLineIndex 间奏锚定的歌词行索引
   * @returns 本次间奏是否有足够时长显示间奏点
   */
  setInterlude(interlude: [MediaTime, MediaTime], currentTime?: MediaTime, forceReset?: boolean, anchorLineIndex?: number): boolean;
  /**
   * 清空间奏区间并终止当前演出
   *
   * 与 {@link dismiss} 的区别在于本方法会一并抹去区间状态，
   * 使此后重新进入同一间奏区间时能够重新演出
   *
   * @param immediate 是否立即隐藏而非淡出
   */
  clearInterlude(immediate?: boolean): void;
  /**
   * 结束间奏点演出，默认使用 150ms 淡出
   * @param immediate 是否立即隐藏
   */
  dismiss(immediate?: boolean): void;
  /**
   * 设置间奏点的变换位置并立即刷新一次
   * @param left 横向位置
   * @param top 纵向位置
   * @param immediate 是否绕过弹簧直接跳转到目标位置，用于触摸拖动等需要跟手的场景
   */
  setTransform(left?: number, top?: number, immediate?: boolean): void;
  pause(): void;
  resume(): void;
  /**
   * 把演出时钟对齐到指定的媒体时间，由宿主每次推送播放进度时调用
   */
  syncClock(time: MediaTime): void;
  /**
   * 逐帧推进演出并把当前帧交给子类渲染
   * @param delta 距离上一次调用的物理时长
   */
  update(delta?: Duration): void;
  /**
   * 释放演出状态
   */
  dispose(): void;
  /**
   * 由子类实现的渲染逻辑
   *
   * 快照对象会被基类逐帧复用，必须在当前帧内消费完毕，不得保留其引用
   *
   * `isActive` 为 `false` 时表示演出已经结束或取消，应当隐藏渲染物
   *
   * @param snapshot 当前帧的视觉状态
   * @param left 由 {@link setTransform} 设置的横向位置
   * @param top 由 {@link setTransform} 设置的纵向位置经 {@link posY} 平滑后的坐标
   */
  protected abstract render(snapshot: Readonly<InterludeDotsSnapshot>, left: number, top: number): void;
  /**
   * 进入演出阶段并确定编排方式
   *
   * 演出重建后的第一帧位置由外部重新给出，不参与弹簧过渡，
   * 因此一并重置 {@link shouldSnapPosY}
   */
  private enterPerforming;
  /**
   * 进入淡出阶段，冻结当前帧的不透明度作为衰减起点
   */
  private enterFading;
  /**
   * 结束演出，回到既不推进时钟也不渲染的静止状态
   */
  private enterIdle;
  /**
   * 取消正在进行的淡出
   *
   * 只清掉淡出阶段，演出阶段保持原样，随后的收尾仍需据此判断
   * 是否有可见内容要派发隐藏快照
   */
  private cancelFadeOut;
  /**
   * 演出被取消时立即隐藏渲染物并清空演出状态
   */
  private hidePerformance;
  /**
   * 写入三颗圆点当前帧的不透明度
   *
   * 最终不透明度由点亮分数映射的透明度与各圆点的错峰入场系数相乘得到
   *
   * @param internalMs 距演出开始的经过时间
   * @param fractions 三颗圆点各自的点亮分数（0~1）
   */
  private writeDotOpacities;
  /**
   * 物理淡出步进器
   */
  private updateFadeOut;
  /**
   * 按经过时间求出该时刻的视觉状态
   *
   * @remarks 返回的快照对象是内部复用的同一引用，必须在当前帧内消费完毕
   * @param elapsed 距离本次演出时间锚点的经过时间
   */
  private resolveSnapshot;
}
//#endregion
//#region src/lyric-player/base/layout.d.ts
/**
 * 布局对齐的静态配置项
 *
 * 一般很少改变
 */
interface LayoutConfig {
  /**
   * 自动对齐的锚点
   */
  alignAnchor: LayoutAlignAnchor;
  /**
   * 0.0 - 1.0 的视口相对位置
   */
  alignPosition: number;
  /**
   * 视口上下额外保留的预渲染距离，单位为像素
   */
  overscanPx: number;
}
/**
 * 每一帧都可能变化的排版上下文状态
 */
interface LayoutFrameContext {
  /**
   * 播放器容器当前总高度
   */
  containerHeight: number;
  /**
   * 被滚动引擎钳制后的安全滚动量
   */
  scrollOffset: number;
  /**
   * 当前视口焦点目标
   */
  target: FocalTarget;
  /**
   * 底栏高度
   */
  bottomLineHeight: number;
  /**
   * 间奏点状态，若当前排版帧无间奏则为 undefined
   */
  interlude?: {
    /**
     * 间奏点高度
     */
    totalHeight: number;
    /**
     * 间奏点应挂在第几行之后
     * -1 表示首行上方，0 ~ N-1 表示对应行下方
     **/
    anchorIndex: number;
  };
}
/**
 * 单行歌词的渲染几何指令
 */
interface RenderInstruction {
  /**
   * 当前行歌词的 Y 轴绝对目标坐标
   */
  y: number;
  /**
   * 当前行歌词的高
   *
   * 可能为测量值或估算值
   */
  height: number;
  /**
   * 当前是否在可视区域（含 Overscan 容差）内，用于剔除渲染
   */
  isInViewport: boolean;
}
/**
 * 排版计算返回的复合结果
 */
interface LayoutResult {
  /**
   * 本次排版计算的有效歌词行数
   */
  lineCount: number;
  /**
   * 对应所有歌词行的渲染指令池
   * @remarks 指令池由 LayoutCalculator 复用，遍历时务必以 {@link lineCount} 为界
   */
  readonly lineInstructions: ReadonlyArray<RenderInstruction>;
  /**
   * 是否需要显示间奏点
   */
  hasInterlude: boolean;
  /**
   * 如果需要显示间奏点，它的 Y 轴绝对坐标
   */
  interludeY: number;
  /**
   * 底栏的 Y 轴绝对坐标
   */
  bottomLineY: number;
  /**
   * 底栏是否在可视范围内，用于剔除渲染
   */
  isBottomLineInViewport: boolean;
}
/**
 * 决定排版原点焦点的目标
 */
type FocalTarget = {
  type: "line";
  index: number;
} | {
  type: "interlude";
  anchorIndex: number;
} | {
  type: "bottom";
};
type ResolvedLayoutMetrics = {
  isValid: boolean;
  focalTopY: number;
  anchorOffset: number;
  interludeTotalHeight: number;
  activeInterludeAnchor: number | undefined;
};
type LayoutFrameSession = ResolvedLayoutMetrics & {
  containerHeight: number;
  alignPosition: number;
  overscanPx: number;
  bottomLineHeight: number;
};
declare class LayoutCalculator {
  /**
   * 前缀和缓存
   *
   * 长度为歌词行数 + 1
   *
   * prefixSums[i] 存储的是第 0 行到第 i-1 行的总高度，不包含间奏点
   */
  private prefixSums;
  private heights;
  /**
   * 使用 Uint8Array 作为掩码，1 表示该行经历了真实测量，0 表示该行在使用 fallback 高度
   */
  private isMeasured;
  /**
   * 渲染指令对象池
   *
   * 长度永远只会增加不会减少，避免 GC
   */
  private instructionPool;
  private isPrefixSumDirty;
  private readonly resolvedMetrics;
  /**
   * 全局唯一复用的返回结果实例
   */
  private readonly layoutResult;
  /**
   * 缓存的歌词行数
   */
  private lyricCount;
  /**
   * 缓存的所有歌词总高度
   */
  private totalLyricHeight;
  /**
   * 初始化排版空间结构与高度缓存
   *
   * 在加载新歌词时调用
   *
   * @param count 歌词总行数
   * @param defaultHeight 尚未渲染/测量的行的默认回退高度
   */
  initHeights(count: number, defaultHeight: number): void;
  /**
   * 获取指定索引歌词行的计算高度
   * @remarks 可能为测量值或估算值
   * @param index 歌词行索引
   */
  getLineHeight(index: number): number;
  /**
   * 设置单行歌词的真实测量高度
   * @param index 歌词行索引
   * @param height 真实测量的高度
   */
  setLineHeight(index: number, height: number): void;
  /**
   * 批量更新所有未测量行的回退高度
   *
   * 仅在容器 Resize 等会导致回退基准（如 containerHeight / 5）发生变化时调用。
   * 已被真实测量的行不受影响。
   *
   * @param defaultHeight 新的默认回退高度
   */
  updateUnmeasuredHeights(defaultHeight: number): void;
  /**
   * 解析焦点度量并计算物理滚动边界
   *
   * 返回滚动安全闭区间 `{ min, max }` 以及此帧的生命周期会话句柄 {@link LayoutFrameSession}
   * 供后续使用 `ScrollInteractionEngine.updateBoundary` 钳制 `scrollOffset` 后传给 {@link commit}
   *
   * @param ctx 动态帧上下文，包含当前容器尺寸、焦点目标与底栏高度
   * @param config 布局静态配置，包含对齐锚点、相对位置与 Overscan 容差
   */
  beginFrame(ctx: LayoutFrameContext, config: LayoutConfig): {
    bounds: {
      min: number;
      max: number;
    };
    session: LayoutFrameSession;
  };
  /**
   * 基于已钳制的 `scrollOffset` 和第一阶段的 {@link LayoutFrameSession} 提交排版并生成指令
   *
   * @param session 由 {@link beginFrame} 生成的单帧排版会话
   * @param scrollOffset 经过边界钳制后的安全滚动偏移量
   *
   * @returns 复用的排版结果实例 {@link LayoutResult}
   */
  commit(session: LayoutFrameSession, scrollOffset: number): LayoutResult;
  /**
   * 解析当前排版帧所对应的间奏点挂载锚点行索引
   *
   * @param interlude 当前时间线处于激活状态的间奏点信息
   * @param focalTarget 当前帧对齐的物理焦点
   */
  static resolveInterludeAnchorIndex(interlude: {
    anchorLineIndex: number;
  } | undefined, focalTarget: FocalTarget): number | undefined;
  /**
   * 如果高度发生过改变，则重新计算前缀和
   */
  private ensurePrefixSums;
  /**
   * 公共的焦点度量与范围校验逻辑
   */
  private resolveLayoutMetrics;
  /**
   * 测量对齐目标的几何信息
   */
  private updateFocalMetrics;
  /**
   * 根据锚点与目标高度计算内部相对偏移
   */
  private calculateAnchorOffset;
  /**
   * 重置排版结果为安全干净的状态（全部不可见/无底栏）
   */
  private resetLayoutResult;
}
//#endregion
//#region src/lyric-player/base/lyric-data-manager.d.ts
/**
 * 所有歌词行数据的配置选项，包括歌词行优化和掩码选项
 */
interface LyricDataConfig {
  optimizeOptions?: OptimizeLyricOptions;
  maskMode?: MaskObsceneWordsMode;
  maskChar?: string;
}
/**
 * 歌词数据管理器
 *
 * 负责：
 * 1. 存储原始歌词、进行深拷贝、应用歌词优化、应用歌词掩码
 * 2. 计算是否为动态歌词 (逐字歌词) 和是否有对唱歌词
 */
declare class LyricDataManager {
  /**
   * 原始的歌词行数组
   */
  private rawLines;
  /**
   * 经过处理后的歌词行数组
   */
  private processedLines;
  /**
   * 当前处理好的 processedLines 是否已经失效
   */
  private isDirty;
  private optimizeOpts;
  private maskMode;
  private maskChar;
  private isNonDynamic;
  private hasDuetLine;
  setOriginalLines(lines: LyricLine[]): void;
  setConfig(config: LyricDataConfig): void;
  /**
   * 获取用户设置的原始歌词行数组，未经过优化和掩码
   * @returns 原始歌词行数组
   */
  getRawLines(): ReadonlyArray<LyricLine>;
  /**
   * 获取对原始歌词行数组处理过的的歌词行数组
   * @returns 经过处理的歌词行数组
   */
  getProcessedLines(): ReadonlyArray<LyricLine>;
  /**
   * 歌词是否为非动态歌词，又称为逐行歌词
   */
  getIsNonDynamic(): boolean;
  /**
   * 歌词是否包含对唱歌词
   */
  getHasDuetLine(): boolean;
  getOptimizeOptions(): OptimizeLyricOptions;
  getMaskMode(): MaskObsceneWordsMode;
  getMaskChar(): string;
  private ensurePipeline;
  private processPipeline;
  private applyMask;
}
//#endregion
//#region src/lyric-player/base/scroll.d.ts
type ScrollInputType = "touch" | "wheel";
interface ScrollEngineHooks {
  /**
   * 当滚动偏移量发生变化时高频触发
   *
   * @param isContinuous 当前是否为连续输入 (触摸或惯性 RAF)
   *
   * 一般的使用场景为：
   * * 若是连续输入，上层应绕过弹簧动画效果直接照着偏移量重新布局；
   * * 若是低频离散步进 (如滚轮)，则上层应保留弹簧效果以便在离散步进之间展示平滑的动画
   */
  onScrollUpdate: (isContinuous: boolean) => void;
  /**
   * 用户明确产生滑动意图 (手指触摸超过 10px) 或使用滚轮时触发
   *
   * 一般在此暂停模糊效果、暂停自动跟随等
   */
  onInteractionStart: (type: ScrollInputType) => void;
}
declare class ScrollInteractionEngine {
  private container;
  private hooks;
  private offset;
  private minOffset;
  private maxOffset;
  private touchState;
  /**
   * 记录 Touch 开始时被打断的既有状态
   */
  private interruptedState;
  private inertiaRafId;
  private scrollEndTime;
  private isInteracting;
  private wheelEndTimeoutId;
  private abortController;
  constructor(container: HTMLElement, hooks: ScrollEngineHooks);
  private startInteraction;
  private endInteraction;
  private onTouchStart;
  private onTouchMove;
  private onTouchCancel;
  private onTouchEnd;
  private onWheel;
  /**
   * 将给定的偏移量限制在当前的 [minOffset, maxOffset] 边界内
   */
  private clampOffset;
  /**
   * 滚动结束是否已超过 {@link AUTO_ALIGN_RESUME_DELAY_MS}
   */
  get canResumeAutoAlign(): boolean;
  /**
   * 更新允许的滚动边界，并返回钳制后的实际 offset
   *
   * @description
   * 在歌词发生排版变化，如页面 `resize`、加载了新歌词、展开背景歌词、歌词行高度改变时，
   * 计算出当前视口内允许滚动的上限与下限，通过此方法传给滚动引擎以确保滚动不会越界
   */
  updateBoundary(min: number, max: number): number;
  /**
   * 覆盖当前的滚动偏移量并终止正在进行的惯性动画或等待状态
   *
   * @description
   * 当需要清空全部用户手势带来的临时滚动状态时，例如，用户点击了某行歌词触发了
   * Seek、歌曲切歌、或者焦点切换时恢复自动对齐，调用此方法重置滚动引擎
   */
  resetScroll(targetOffset?: number): void;
  private clearTimers;
  dispose(): void;
}
//#endregion
//#region src/lyric-player/base/seek-detector.d.ts
/**
 * 读取单调递增的物理时钟的函数，单位为毫秒
 *
 * 默认使用 `performance.now`
 */
type WallClock = () => number;
/**
 * 跳转状态自动推导器
 *
 * 让下游使用者只推送播放进度、无需自己判断某次进度变化是否为跳转
 *
 * 判定的思路是把本次媒体时钟的推进量与当前播放状态下它应有的推进量比较，
 * 超出容差的偏离即视为跳转。判定规则为：
 * - 进度倒退视为跳转
 * - 播放时应有的推进量是物理时钟的推进量，容差随之按比例放宽
 * - 暂停时应有的推进量是零，容差只留固定底限
 */
declare class SeekDetector {
  private readonly now;
  private lastMediaTime;
  private lastWallTime;
  private hasBaseline;
  /**
   * @param now 物理时钟读取函数，默认为 `performance.now`
   */
  constructor(now?: WallClock);
  /**
   * 推导本次进度变化是否为跳转
   *
   * @param time 下游推送的当前播放进度
   * @param isPlaying 当前是否在播放，决定本次推送应有的推进量与容差
   * @returns 是否应当按跳转处理
   */
  detect(time: MediaTime, isPlaying: boolean): boolean;
  reset(): void;
  private rebase;
}
//#endregion
//#region src/lyric-player/base/timeline.d.ts
/**
 * 用于进度计算的最小歌词数据
 */
interface TimeBounds {
  readonly startTime: MediaTime;
  readonly endTime: MediaTime;
}
/**
 * 当前命中的间奏区间信息
 */
interface PlayerInterlude {
  /**
   * 间奏开始时间，即此前全部歌词行中最晚的结束时间
   */
  readonly startTime: MediaTime;
  /**
   * 间奏结束时间，即间奏后第一行歌词的开始时间
   */
  readonly endTime: MediaTime;
  /**
   * 间奏点应插入的位置基准
   *
   * 即显示顺序上间奏前的最后一行歌词的索引，`-1` 表示第一句之前
   */
  readonly anchorLineIndex: number;
}
/**
 * 当前播放时间线状态的只读快照
 *
 * 用于给 UI 执行排版和计算各种歌词行的效果
 *
 * @remarks
 * 在获取快照后，必须在同一帧内消费完毕，切勿保留其引用，因为下一帧就会被原地覆写
 */
interface TimelineSnapshot {
  /**
   * 当前时间推导所依据的绝对播放时间
   *
   * 一般用于提供给 UI 层在绘制/排版阶段作为基准时间
   *
   * 例如给 InterludeDotsBase 计算播放动画的当前时间戳
   */
  readonly currentTime: MediaTime;
  /**
   * 当前进度命中的、正在播放的歌词组
   */
  readonly playingGroups: ReadonlySet<number>;
  /**
   * 当前进度命中的、正在高亮的歌词组
   *
   * 高亮的歌词组可能会多于正在播放的，例如一行歌词唱完后不会自行熄灭，而是保持高亮直到下一行开始播放
   *
   * 因此多行重叠时先唱完的行会陪着后面的行一起亮，句间空隙内上一行也保持高亮以维持可读性
   */
  readonly highlightedGroups: ReadonlySet<number>;
  /**
   * 自动滚动应该对齐到哪一行歌词
   *
   * 取值始终落在 `[0, 歌词行数 - 1]` 内，没有歌词时为 `0`，因此可以直接用于索引
   *
   * @remarks
   * 仅在 {@link isFocusOnInterlude} 为 `false` 时有效
   *
   * 间奏期间此值仍然指向间奏前的那一组歌词，此时应当改为对齐 {@link activeInterlude} 的间奏点
   */
  readonly scrollToIndex: number;
  /**
   * 处于高亮状态的歌词行中，最靠后的一行
   *
   * 例如，如果当前有高亮行索引 `[1, 2, 3]`，则 `latestHighlightedIndex` 为 `3`
   *
   * 如果当前没有任何高亮行，被置为 undefined，如首行开始之前、间奏区间内、
   * 歌曲播放完毕、或已开始的行均为零时长的情况
   */
  readonly latestHighlightedIndex?: number;
  /**
   * 标识歌曲是否播放完毕，即当前时间已经越过全部歌词行中最晚的结束时间
   *
   * 此时所有高亮都已被清空，一般用于展示底栏
   */
  readonly isEndOfSong: boolean;
  /**
   * 当前命中的间奏区间数据
   */
  readonly activeInterlude?: PlayerInterlude;
  /**
   * 当前是否应当聚焦在间奏点上
   */
  readonly isFocusOnInterlude: boolean;
}
/**
 * 时间线增量变化
 *
 * 主要用于让 UI 层能够以 $O(1)$ 到 $O(K)$ 的开销知道当前这一帧相比上一帧改变了什么，
 * 而不需要去遍历或比对全量状态
 */
interface TimelineDiff {
  /**
   * 当前帧是否有任何实质性的状态变更，如播放行更替、高亮行新增/移除、间奏状态切换、焦点切换，或发生了跳转
   *
   * UI 层接收到 diff 后，检查此标志即可直接跳过后续所有的布局计算
   */
  readonly hasChanged: boolean;
  /**
   * 标识本次同步的时间轴是否发生了跳转
   *
   * 在显式跳转 (`sync` 的 `forceSeek`) 或时间倒退时为 `true`
   *
   * 例如跳转时使用更缓慢的弹簧参数 (参见 `spring.ts`)，
   * 以及在非触摸状态下重置滚动坐标系
   */
  readonly isTimeJumped: boolean;
  /**
   * 在当前时间进度下，最新被命中的、正在播放的歌词索引列表
   *
   * 用于通知 UI 哪些歌词行开始播放了
   */
  readonly addedPlaying: ReadonlyArray<number>;
  /**
   * 在当前时间进度下，刚刚脱离正在播放状态的歌词索引列表，
   * 即上一帧还在播放、但本帧已不再命中其 `[startTime, endTime)` 的歌词行
   *
   * 正常播放时是时间越过了 `endTime`，跳转时也可能是跳到了 `startTime` 之前
   *
   * 用于通知 UI 侧哪些歌词行已结束，后续可能会转入高亮行以保持高亮状态
   */
  readonly removedPlaying: ReadonlyArray<number>;
  /**
   * 在当前帧中，需要变成高亮行的歌词索引列表
   *
   * 一般用于 UI 遍历并启用歌词行
   */
  readonly addedHighlighted: ReadonlyArray<number>;
  /**
   * 在当前帧中，不再是高亮行的歌词行索引列表
   *
   * 一般用于 UI 遍历并停用歌词行
   */
  readonly removedHighlighted: ReadonlyArray<number>;
  /**
   * 标识间奏状态是否发生了切换
   *
   * 例如：刚刚进入间奏区间、刚刚离开间奏区间、或从一个间奏区间跳到了另一个间奏区间
   *
   * 用于通知 UI 是否需要重新计算间奏点的位置和动画
   */
  readonly isInterludeChanged: boolean;
  /**
   * 标识自动对齐的目标歌词行索引是否发生了变化
   *
   * 用于通知 UI 需要移动到新的歌词行
   */
  readonly isScrollToChanged: boolean;
  /**
   * 标识歌曲播放完毕状态是否发生了变化
   */
  readonly isEndOfSongChanged: boolean;
}
declare class TimelineController {
  private lyricBounds;
  /**
   * 全部歌词行中最晚的结束时间，用于判定歌曲是否播放完毕
   */
  private maxEndTime;
  /**
   * 预先计算的间奏区域
   */
  private precalculatedInterludes;
  /**
   * 保存上次顺序扫描停止的位置，用于避免每次都从头遍历所有歌词，提高性能
   */
  private playbackCursor;
  private interludeCursor;
  private playingGroupsSet;
  private highlightedGroupsSet;
  private nextPlayingSet;
  private nextHighlightedSet;
  private addedPlayingIds;
  private removedPlayingIds;
  private addedHighlightedIds;
  private removedHighlightedIds;
  private expiredHighlightedIds;
  private readonly snapshot;
  private readonly diff;
  /**
   * 提前设置好歌词的时间数据，内部会根据此数据来进行时间线推导，同时预计算间奏区间
   *
   * @param bounds 歌词行的时间边界，**必须按 `startTime` 升序排列**，不按 `startTime`
   * 排列可能会导致时间推导出现意外情况
   */
  setTimeBounds(bounds: readonly TimeBounds[]): void;
  /**
   * 获取当前播放时间线状态的只读快照
   *
   * 用于给 UI 执行排版和计算各种歌词行的效果
   *
   * @remarks 在获取快照后，必须在同一帧内消费完毕，切勿保留其引用，因为下一帧就会被原地覆写
   * @returns 时间线快照
   */
  getSnapshot(): TimelineSnapshot;
  /**
   * 将播放进度推进到指定时间，并返回相对上一帧的增量变化
   *
   * @remarks
   * 歌词行的高亮生命周期为：
   * * 命中 `[startTime, endTime)` 时高亮
   * * 唱完后不会自行熄灭，而是继续保持高亮
   *
   * 直到出现下列任一情况：
   *
   * 1. 有新的歌词行开始播放，此时已唱完的行被一起熄灭
   * 2. 进入间奏区间，此时清空全部高亮并把焦点交给间奏点
   * 3. 歌曲播放完毕，此时清空全部高亮并设置 {@link TimelineSnapshot.isEndOfSong} 为 true
   * 4. 发生跳转，此时按跳转后的时间重新推导
   *
   * @param time 当前播放时间
   * @param forceSeek 这次时间变化是否由跳转触发
   * @returns 相对上一帧的增量变化
   */
  sync(time: MediaTime, forceSeek?: boolean): TimelineDiff;
  /**
   * 处理正常播放时的时间线推导
   */
  private performPlayback;
  /**
   * 处理 Seek 时的时间线推导
   *
   * 直接按目标时间重建播放与高亮行集合，结果与正常播放到该时刻时一致
   *
   * @param time 跳转到的时间
   * @param dropLingeringWhenIdle 在没有任何行正在播放时，是否丢弃那些已经唱完、
   * 但在正常播放中仍会保持高亮的行，用于在间奏和播放完时清空高亮行
   */
  private performSeek;
  /**
   * 把 Seek 重建出的目标集合与上一帧的集合求对称差，输出发生变化的部分
   */
  private commitSeekDiff;
  /**
   * 立即熄灭当前全部高亮歌词行
   *
   * 用于间奏与曲末这两个没有下一行接续、但必须清空高亮的场景
   */
  private flushAllHighlighted;
  /**
   * 预计算全部间奏区间
   * @param bounds 按 `startTime` 升序排列的歌词时间边界
   * @returns 按时间升序排列、互不重叠的间奏区间，供二分查找与游标推进使用
   */
  private calculateInterludes;
  /**
   * 查找当前时间命中的间奏区间，并顺带推进或重定位间奏游标
   * @param time 当前播放时间
   * @param isSeek 当前帧是否为跳转
   * @returns 命中的间奏区间，未命中时为 undefined
   */
  private resolveActiveInterlude;
  /**
   * 根据间奏命中情况和当前高亮状态推导是否应当聚焦间奏点
   *
   * 处于间奏区域且没有任何歌词高亮时聚焦间奏点，否则交还给歌词行
   *
   * @param activeInterlude 当前命中的间奏区间
   */
  private updateInterludeFocus;
  /**
   * 清空全部推导状态，回到时间原点
   */
  private reset;
}
//#endregion
//#region src/lyric-player/base/index.d.ts
/**
 * 播放器布局状态。
 *
 * 记录当前视口动态测量得到的尺寸
 */
interface PlayerLayoutState {
  /** 间奏点元素当前测量得到的尺寸 */
  interludeDotsSize: [number, number];
}
/**
 * 播放器滚动状态。
 *
 * 记录用户的滚动行为
 */
interface PlayerScrollState {
  /** 是否处于用户滚动过，尚未回归自动对齐的状态 */
  isAutoAlignSuspended: boolean;
  isTouchScrolled: boolean;
}
/**
 * 歌词播放器的基类，已经包含了有关歌词操作和排版的功能，
 * 子类需要为其实现对应的显示展示操作
 */
declare abstract class LyricPlayerBase extends EventTarget implements HasElement, Disposable {
  protected element: HTMLElement;
  abstract get baseFontSize(): number;
  protected isPlaying: boolean;
  protected timelineController: TimelineController;
  protected seekDetector: SeekDetector;
  protected enableAutoSeekDetection: boolean;
  private hasBottomContent;
  private bottomLineObserver;
  /** @internal */
  lyricGroupElementMap: WeakMap<Element, LyricLineGroupBase>;
  protected lyricLinesIndexes: WeakMap<LyricLineBase, number>;
  protected disableSpring: boolean;
  protected dataManager: LyricDataManager;
  protected get processedLines(): ReadonlyArray<LyricLine>;
  protected get isNonDynamic(): boolean;
  protected get hasDuetLine(): boolean;
  protected layoutState: PlayerLayoutState;
  protected layoutConfig: LayoutConfig;
  /** LayoutCalculator 所使用的排版上下文状态 */
  private frameContext;
  /**
   * 逐帧刷新的视觉推导派生值
   *
   * 存放会在逐行循环内被反复求值的量，避免逐行重算
   */
  private visualFrame;
  protected interludeDots: InterludeDotsBase & HasElement;
  protected bottomLine: BottomLine;
  protected enableBlur: boolean;
  protected enableScale: boolean;
  protected hidePassedLines: boolean;
  protected scrollEngine: ScrollInteractionEngine;
  protected scrollState: PlayerScrollState;
  protected layoutCalculator: LayoutCalculator;
  private focusController;
  currentLyricGroups: LyricLineGroupBase[];
  lyricGroupSize: WeakMap<LyricLineGroupBase, [number, number]>;
  readonly size: [number, number];
  protected isPageVisible: boolean;
  /** 默认/回退单行歌词估算高度基准 (containerHeight / 5) */
  get defaultLineHeight(): number;
  /**
   * 获取指定索引歌词行的高度
   * @remarks 可能为测量值或估算值
   */
  getLineHeight(index: number): number;
  /** 是否强制让背景人声行始终后置（即始终在主歌词下方显示，不前置背景人声） */
  protected alwaysPostpositionBackground: boolean;
  protected posXSpringParams: Partial<SpringParams>;
  protected posYSpringParams: Partial<SpringParams>;
  protected scaleSpringParams: Partial<SpringParams>;
  protected scaleForBGSpringParams: Partial<SpringParams>;
  private lyricGroupIndexMap;
  private onPageShow;
  private onPageHide;
  private onVisibilityChange;
  /** @internal */
  resizeObserver: ResizeObserver;
  protected wordFadeWidth: number;
  constructor(element?: HTMLElement);
  /**
   * 设置文字动画的渐变宽度，单位以歌词行的主文字字体大小的倍数为单位，默认为 0.5，即一个全角字符的一半宽度
   *
   * 如果要模拟 Apple Music for Android 的效果，可以设置为 1
   *
   * 如果要模拟 Apple Music for iPad 的效果，可以设置为 0.5
   *
   * 如果想要近乎禁用渐变效果，可以设置成非常接近 0 的小数（例如 `0.0001` ），但是**不可以为 0**
   *
   * @param value 需要设置的渐变宽度，单位以歌词行的主文字字体大小的倍数为单位，默认为 0.5
   */
  setWordFadeWidth(value?: number): void;
  /**
   * 是否启用歌词行缩放效果，默认启用
   *
   * 如果启用，非选中的歌词行会轻微缩小以凸显当前播放歌词行效果
   *
   * 此效果对性能影响微乎其微，推荐启用
   * @param enable 是否启用歌词行缩放效果
   */
  setEnableScale(enable?: boolean): void;
  /**
   * 获取当前是否启用了歌词行缩放效果
   * @returns 是否启用歌词行缩放效果
   */
  getEnableScale(): boolean;
  /**
   * 获取当前文字动画的渐变宽度，单位以歌词行的主文字字体大小的倍数为单位
   * @returns 当前文字动画的渐变宽度，单位以歌词行的主文字字体大小的倍数为单位
   */
  getWordFadeWidth(): number;
  /**
   * 设置持续性的跳转状态
   *
   * @deprecated 此方法已无实际作用，调用它不会产生任何效果，仅为兼容保留，将在未来移除
   *
   * 跳转状态现在由每次进度推送逐帧推导，不再存在需要外部显式解除的持续状态
   *
   * 跳转会通过以下三条途径被识别，三者同时生效：
   *
   * - {@link setCurrentTime} 的 `isSeek` 参数，由调用方明确告知某次进度变化是跳转
   * - 进度倒退，由内部无条件识别，不受任何开关控制
   * - 自动推导，由内部比对进度的实际推进量与它应有的推进量识别超量前进，默认启用，
   *   可通过 {@link setEnableAutoSeekDetection} 关闭
   */
  setIsSeeking(_isSeeking: boolean): void;
  /**
   * 设置是否自动推导跳转状态，默认启用
   *
   * 启用后，即使调用 {@link setCurrentTime} 时没有传入 `isSeek`，
   * 内部也会在进度前进时比较它的实际推进量与应有的推进量，超量前进即视为跳转
   *
   * 应有的推进量取决于当前的播放状态，因此请按 {@link pause} 与 {@link resume}
   * 的文档正确同步播放状态：
   * - 播放时以物理时钟的推进量为准，容差随之按比例放宽，以容纳倍速播放与不均匀的推送节奏
   * - 暂停时进度本不该前进，应有的推进量是零，因此任何超出抖动幅度的前进都会被视为跳转
   *
   * 这意味着进度信息的粒度粗于推送间隔时，粒度跳变的那一帧会因超量前进被视为跳转，
   * 此时应当改善进度来源的精度，或关闭此开关
   *
   * 较小的向前跳转可能无法被识别，但一般影响不大
   *
   * 此开关只控制上述超量前进的判定。进度倒退不受它控制，无论是否启用推导都会被视为跳转；
   * 进度保持不变则既不前进也不后退，不会被推导视为跳转
   *
   * 推导只会额外识别出跳转，不会否决已显式传入的跳转标志，因此如果你已经在正确传入
   * 跳转标志了，则一般无需关心此开关。若你的进度来源精度很差而导致超量前进被频繁误判，
   * 可以选择关闭
   *
   * @param enable 是否启用自动推导
   */
  setEnableAutoSeekDetection(enable?: boolean): void;
  /**
   * 获取当前是否启用了跳转状态的自动推导
   * @returns 是否启用自动推导
   */
  getEnableAutoSeekDetection(): boolean;
  /**
   * 设置是否隐藏已经播放过的歌词行，默认不隐藏
   * @param hide 是否隐藏已经播放过的歌词行，默认不隐藏
   */
  setHidePassedLines(hide: boolean): void;
  /**
   * 设置是否启用歌词行的模糊效果
   * @param enable 是否启用
   */
  setEnableBlur(enable: boolean): void;
  /**
   * 批量更新歌词处理配置，包括优化和掩码设置
   *
   * @remarks
   * 此方法不会自动重建歌词行和刷新视图，
   * 适用于在渲染前预设配置、批量初始化，或需要手动控制 DOM 刷新时机的场景
   * @param config 需要更新的配置集合
   * @see {@link LyricDataConfig}
   */
  setLyricProcessConfig(config: LyricDataConfig): void;
  /**
   * 批量更新歌词处理配置，包括优化和掩码设置
   *
   * 可以调用此方法以避免多次单独设置处理配置导致的多次刷新开销
   * @remarks 在设置完成后会自动重建歌词行和刷新视图
   * @param config 需要更新的配置集合
   * @see {@link LyricDataConfig}
   */
  updateLyricProcessConfig(config: LyricDataConfig): void;
  /**
   * 设置歌词中不雅用语的掩码模式
   * @remarks 在设置完成后会自动重建歌词行和刷新视图
   * @param mode 掩码模式
   * @see {@link MaskObsceneWordsMode}
   */
  setMaskObsceneWords(mode: MaskObsceneWordsMode): void;
  /**
   * 设置不雅用语掩码使用的字符，默认为 `*`
   * @remarks 在设置完成后会自动重建歌词行和刷新视图
   * @param char 单个字符，用于替换不雅用语中的字符
   */
  setMaskObsceneWordChar(char: string): void;
  /**
   * 设置歌词的优化配置项，这些配置项默认全部开启
   * @remarks 在设置完成后会自动重建歌词行和刷新视图
   * @param options 优化配置选项
   * @see {@link OptimizeLyricOptions}
   */
  setOptimizeOptions(options: OptimizeLyricOptions): void;
  rebuildLyricLines(): void;
  /**
   * 设置目标歌词行的对齐方式，默认为 `center`
   *
   * - 设置成 `top` 的话将会向目标歌词行的顶部对齐
   * - 设置成 `bottom` 的话将会向目标歌词行的底部对齐
   * - 设置成 `center` 的话将会向目标歌词行的垂直中心对齐
   * @param alignAnchor 歌词行对齐方式，详情见函数说明
   */
  setAlignAnchor(alignAnchor: LayoutAlignAnchor): void;
  /**
   * 设置默认的歌词行对齐位置，相对于整个歌词播放组件的大小位置，默认为 `0.5`
   * @param alignPosition 一个 `[0.0-1.0]` 之间的任意数字，代表组件高度由上到下的比例位置
   */
  setAlignPosition(alignPosition: number): void;
  /**
   * 设置 overscan（视图上下额外缓冲渲染区）距离，单位：像素。
   * @param px 像素值，默认 300
   */
  setOverscanPx(px: number): void;
  /** 获取当前 overscan 像素距离 */
  getOverscanPx(): number;
  /**
   * 设置是否使用物理弹簧算法实现歌词动画效果，默认启用
   *
   * 如果启用，则会通过弹簧算法实时处理歌词位置，但是需要性能足够强劲的电脑方可流畅运行
   *
   * 如果不启用，则会回退到基于 `transition` 的过渡效果，对低性能的机器比较友好，但是效果会比较单一
   */
  setEnableSpring(enable?: boolean): void;
  /**
   * 获取当前是否启用了物理弹簧
   * @returns 是否启用物理弹簧
   */
  getEnableSpring(): boolean;
  /**
   * 设置当前播放歌词，要注意传入后这个数组内的信息不得修改，否则会发生错误
   * @param lines 歌词数组
   * @param initialTime 初始时间，默认为 0
   * @throws {TypeError} 歌词时间戳不是有限的非负数字
   * @throws {RangeError} 任一行、单词或注音的开始时间晚于结束时间
   */
  setLyricLines(lines: LyricLine[], initialTime?: number): void;
  /**
   * 获取当前是否在播放
   * @returns 当前是否在播放
   */
  getIsPlaying(): boolean;
  /**
   * 设置当前播放进度，此时将会更新内部的歌词进度信息。
   *
   * 内部会根据调用间隔和播放进度自动决定应如何滚动和显示歌词，所以此方法的调用频率越快越准确越好。
   * 调用频率较低或进度细度过粗可能会导致歌词显示延迟或导致自动跳转推导错误。
   * 调用完成后，应每帧调用 {@link update} 方法来执行歌词动画效果。此函数本身不会触发动画效果。
   *
   * 当 `isSeek` 为 `true` 时，将强制按跳转处理，并在下次调用 {@link update} 时触发一系列的行为变更，
   * 具体请参考 <https://amll.dev/guides/component/seeking>，因此请只在真正跳转时设为 `true`
   *
   * @param time 当前播放进度，单位为毫秒，非有限值会被静默忽略
   * @param isSeek 是否强制按跳转处理，默认交由内部推导
   * @see {@link setEnableAutoSeekDetection} 自动推导跳转状态的文档
   * @see https://amll.dev/guides/component/sequence#播放进度
   */
  setCurrentTime(time: number, isSeek?: boolean): void;
  /**
   * 推进时间线并把增量变化应用到视图上
   * @param mediaTime 当前播放进度
   * @param isSeek 这次进度变化是否为跳转
   */
  private syncTime;
  /**
   * 由子类实现的歌词组构建逻辑
   */
  protected abstract buildLyricGroups(): void;
  /**
   * 由子类实现的间奏点组件创建逻辑
   *
   * 在基类构造函数中调用，子类需要返回对应渲染实现的后端
   *
   * @remarks 工厂执行时子类字段尚未初始化，实现内不得读取子类实例状态，
   * 所需资源应由返回的组件自行创建或延迟获取
   */
  protected abstract createInterludeDots(): InterludeDotsBase & HasElement;
  /**
   * 由子类实现的底栏组件创建逻辑
   *
   * 在基类构造函数中调用，子类需要返回对应渲染实现的实例
   *
   * @remarks 工厂执行时子类字段尚未初始化，实现内不得读取子类实例状态，
   * 所需资源应由返回的组件自行创建或延迟获取
   */
  protected abstract createBottomLine(): BottomLine;
  /**
   * 重新构建歌词行和时间状态
   *
   * 一般用于在调用 {@link setLyricProcessConfig} 更新配置后手动刷新视图，
   * 或在外部样式/DOM 结构发生改变后重置歌词视图
   *
   * @param initialTime 重建后对齐的初始时间（毫秒），默认使用当前播放进度
   */
  rebuildLyricView(initialTime?: number): void;
  /**
   * 更新歌词纵向滚动动画的弹簧参数。
   *
   * 其策略为：
   * - seeking 或间奏时使用更稳定的固定参数
   * - 普通播放时根据相邻歌词的时间间隔动态调整 stiffness / damping
   * - 播放完毕时使用中速弹簧参数
   *
   * @param isInterludeActive 当前是否命中间奏区间
   * @param isSeeking 本次同步的时间轴是否发生了跳转
   * @param isEndOfSong 歌曲是否播放完毕
   */
  private updateSpringParams;
  /**
   * 重新计算歌词行的几何排版坐标与视觉状态
   *
   * 此方法不会触发 DOM 强制重排
   *
   * 计算完成后，在每一帧调用 `update()` / `commitChanges()` 即可让歌词平滑移动至目标位置
   *
   * @internal 仅供内部和绑定包使用
   * @param reason 触发排版布局更新的原因场景
   */
  calcLayout(reason: LayoutReason): void;
  /**
   * 推导某一行是否处于激活状态
   * @param index 歌词行索引
   * @param snapshot 当前帧的时间线快照
   */
  private resolveIsActive;
  /**
   * 推导一行的目标透明度
   * @param index 歌词行索引
   * @param isInViewport 该行是否在可视区域内
   * @param snapshot 当前帧的时间线快照
   */
  private resolveOpacity;
  /**
   * 按距焦点的行距推导模糊档位
   * @param index 歌词行索引，底栏传入歌词总行数
   * @param isFocused 该目标是否为当前焦点，歌词行传 `isActive`，底栏传是否聚焦底栏
   * @param isInViewport 该目标是否在可视区域内
   * @param snapshot 当前帧的时间线快照
   */
  private resolveBlurLevel;
  /**
   * 设置所有歌词行、底栏和间奏点在横坐标上的弹簧属性，包括重量、弹力和阻力。
   *
   * @param params 需要设置的弹簧属性，提供的属性将会覆盖原来的属性，未提供的属性将会保持原样
   * @deprecated 考虑到横向弹簧效果并不常见，所以这个函数将会在未来的版本中移除
   */
  setLinePosXSpringParams(_params?: Partial<SpringParams>): void;
  /**
   * 设置所有歌词行、底栏和间奏点在​纵坐标上的弹簧属性，包括重量、弹力和阻力。
   *
   * @param params 需要设置的弹簧属性，提供的属性将会覆盖原来的属性，未提供的属性将会保持原样
   */
  setLinePosYSpringParams(params?: Partial<SpringParams>): void;
  /**
   * 设置所有歌词行在​缩放大小上的弹簧属性，包括重量、弹力和阻力。
   *
   * @param params 需要设置的弹簧属性，提供的属性将会覆盖原来的属性，未提供的属性将会保持原样
   */
  setLineScaleSpringParams(params?: Partial<SpringParams>): void;
  /**
   * 暂停部分效果演出，目前会暂停播放间奏点的动画，且将背景歌词显示出来
   */
  pause(): void;
  /**
   * 恢复部分效果演出，目前会恢复播放间奏点的动画
   */
  resume(): void;
  /**
   * 更新动画，这个函数应该被逐帧调用或者在以下情况下调用一次：
   *
   * 1. 刚刚调用完设置歌词函数的时候
   * @param delta 距离上一次被调用到现在的时长，单位为毫秒（可为浮点数）
   */
  update(delta?: number): void;
  protected onResize(): void;
  /**
   * 获取一个特殊的底栏元素，默认是空白的，可以往内部添加任意元素
   *
   * 这个元素始终在歌词的底部，可以用于显示歌曲创作者等信息
   *
   * 但是请勿删除该元素，只能在内部存放元素
   *
   * @returns 一个元素，可以往内部添加任意元素
   */
  getBottomLineElement(): HTMLElement;
  /**
   * 重置用户滚动状态并恢复自动对齐
   *
   * 一个典型的使用场景是在用户滚动完毕、但歌词未自动归位时立刻归位
   */
  resetScroll(): void;
  /**
   * 获取当前播放的、未经过优化和掩码处理的歌词数组
   *
   * 一般和最后调用 `setLyricLines` 给予的参数一样
   * @returns 当前歌词数组
   */
  getLyricLines(): ReadonlyArray<LyricLine>;
  /**
   * 获取当前歌词的播放位置
   *
   * 一般和最后调用 `setCurrentTime` 给予的参数一样
   * @returns 当前播放位置
   */
  getCurrentTime(): number;
  /**
   * 设置是否让背景人声行始终后置显示
   *
   * 默认情况下，如果背景歌词开始时间早于主歌词，会在主歌词上方展示；
   * 如果设置为 `true`，则无论时间顺序如何，背景歌词都会始终在主歌词下方展示
   * @param enable 是否启用始终后置
   */
  setAlwaysPostpositionBackground(enable: boolean): void;
  /** 获取当前是否设置了让背景人声行始终后置显示 */
  getAlwaysPostpositionBackground(): boolean;
  getElement(): HTMLElement;
  dispose(): void;
}
//#endregion
//#region src/lyric-player/dom/interlude-dots.d.ts
declare class InterludeDotsEl extends InterludeDotsBase implements HasElement {
  private element;
  private dot0;
  private dot1;
  private dot2;
  private lastStyle;
  constructor();
  getElement(): HTMLElement;
  protected override render(snapshot: Readonly<InterludeDotsSnapshot>, left: number, top: number): void;
  override dispose(): void;
}
//#endregion
//#region src/lyric-player/dom/lyric-line.d.ts
declare class LyricLineEl extends LyricLineBase {
  private readonly lyricPlayer;
  private readonly lyricLine;
  private readonly element;
  private splittedWords;
  private built;
  private renderMode;
  private maskAnimator?;
  private lastScaleNum;
  private readonly lineHasRubyWords;
  private readonly lineHasRomanWords;
  /**
   * 用于平衡换行、尽量减少各行长度差异的类
   */
  private readonly balancer?;
  constructor(lyricPlayer: DomLyricPlayer, lyricLine?: LyricLine);
  private isEnabled;
  enable(maskAnimationTime?: number, shouldPlay?: boolean): Promise<void>;
  disable(): void;
  private lastWord?;
  resume(): Promise<void>;
  pause(): Promise<void>;
  getLine(): LyricLine;
  show(): void;
  private rebuildStyle;
  override rebuildElement(): void;
  /** 设置翻译与音译行文本 */
  private setSubLinesText;
  private getRubyCharCount;
  private getRubySegments;
  private createWord;
  private buildWord;
  override onLineSizeChange(_size: [number, number]): void;
  updateMaskImageSync(): void;
  getElement(): HTMLElement;
  private setRenderMode;
  override setTransform(scale?: number, opacity?: number, blur?: number, delay?: Duration, mode?: LyricLineRenderMode): void;
  update(delta?: Duration): void;
  override commitChanges(): void;
  /** @internal */
  _getDebugTargetPos(): string;
  private disposeElements;
  override dispose(): void;
}
//#endregion
//#region src/lyric-player/dom/lyric-group.d.ts
declare class LyricLineGroup extends LyricLineGroupBase<LyricLineEl> {
  lyricPlayer: DomLyricPlayer;
  element: HTMLElement;
  bgWrapper?: HTMLElement;
  private lastIsActive?;
  private lastBgHeight;
  private lastBgIsHidden?;
  private lastYNum;
  private lastOpacityNum;
  private lastBlurNum;
  private lastBgSlideYNum;
  constructor(lyricPlayer: DomLyricPlayer, mainLine: LyricLineEl);
  getElement(): HTMLElement;
  isInRenderRange(includeOverscan?: boolean): boolean;
  show(): void;
  hide(): void;
  override update(delta?: Duration): void;
  override commitChanges(): void;
  override onBgSizeChange(size: [number, number]): void;
  addBgLine(bgLine: LyricLineEl): void;
  protected renderStyles(): void;
  override dispose(): void;
}
//#endregion
//#region src/lyric-player/dom/index.d.ts
/**
 * 歌词行鼠标相关事件，可以获取到歌词行的索引、主歌词行以及背景歌词行（如果有）元素
 */
declare class LyricLineMouseEvent extends MouseEvent {
  /**
   * 歌词行索引
   */
  readonly lineIndex: number;
  /**
   * 歌词行元素
   */
  readonly line: LyricLineBase;
  /**
   * 背景人声歌词行元素 (如果存在)
   */
  readonly bgLine: LyricLineBase | undefined;
  /**
   * 自定义标志位，用于记录外部是否调用了 `stopPropagation`
   */
  isPropagationStopped: boolean;
  /** JPOP Corpus Tool patch: 被点中的词元素（带 data-char-start），没点在词上为 null */
  wordElement: Element | null;
  constructor(lineIndex: number, line: LyricLineBase, bgLine: LyricLineBase | undefined, event: MouseEvent);
  override stopPropagation(): void;
  override stopImmediatePropagation(): void;
}
type LyricLineMouseEventListener = (evt: LyricLineMouseEvent) => void;
/**
 * 歌词播放组件，本框架的核心组件
 *
 * 尽可能贴切 Apple Music for iPad 的歌词效果设计，且做了力所能及的优化措施
 */
declare class DomLyricPlayer extends LyricPlayerBase {
  private abortController;
  override currentLyricGroups: LyricLineGroup[];
  override onResize(): void;
  readonly supportPlusLighter: boolean;
  readonly supportMaskImage: boolean;
  readonly innerSize: [number, number];
  private readonly onMouseEventHandler;
  /**
   * 是否为非逐词歌词
   * @internal
   */
  _getIsNonDynamic(): boolean;
  private _baseFontSize;
  get baseFontSize(): number;
  constructor();
  private rebuildStyle;
  override setWordFadeWidth(value?: number): void;
  protected override createInterludeDots(): InterludeDotsEl;
  protected override createBottomLine(): BottomLine;
  /**
   * 重新构建歌词行和时间状态
   *
   * 一般用于在调用 {@link setLyricProcessConfig} 更新配置后手动刷新视图，
   * 或在外部样式/DOM 结构发生改变后重置歌词视图
   *
   * @param initialTime 重建后对齐的初始时间（毫秒），默认使用当前播放进度
   */
  protected override buildLyricGroups(): void;
  override rebuildLyricView(initialTime?: number): void;
  override pause(): void;
  override resume(): void;
  override update(delta?: number): void;
  override dispose(): void;
}
//#endregion
export { AbstractBaseRenderer, BackgroundRender, BaseRenderer, type BottomLine, type BottomLineTransforms, type ColorCount, type ColorVec3, CreatePaletteOptions, type Disposable, DomLyricPlayer, DomLyricPlayer as LyricPlayer, Duration, GLProgram, type GLRenderingContext, type HSVColor, type HasElement, type HistogramSource, type InterludeDotsBase, type InterludeDotsSnapshot, IsolationRenderer, type IsolationRendererOptions, LayoutAlignAnchor, LayoutReason, LayoutReasonStrategyMap, LayoutStrategy, type LyricDataConfig, type LyricLine, type LyricLineBase, LyricLineMouseEvent, LyricLineMouseEventListener, LyricLineRenderMode, LyricPlayerBase, type LyricWord, type LyricWordBase, MAX_FRAME_DELTA, MaskObsceneWordsMode, MediaTime, MeshGradientRenderer, type OptimizeLyricOptions, PaletteAlgorithm, type PaletteIntent, type PaletteResult, PixiRenderer, type ThemeColorResult, buildColorHistogram, channelToLinear, createAutoPalette, createKMeansPalette, createOctTreePalette, createPaletteFromImage, createThemeColor, hsvToRgb, labToRgb, labToXyz, paletteRgbLStarIsDark, paletteRgbLStarIsLight, rgbLStarIsDark, rgbToHsv, rgbToLab, rgbToXyz, spring_d_exports as spring, srgbToOkLab, xyzToLab, xyzToRgb, yToLStar };
//# sourceMappingURL=amll-core.d.mts.map