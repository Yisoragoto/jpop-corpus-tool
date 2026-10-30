import AppKit
import JPCore
import JPCorpusModel
import SwiftUI
import UniformTypeIdentifiers

/// 词云。和 Windows 版同一种观感：整块铺满歌手照片的重度虚化（没有照片时是蓝紫两团光），
/// 头上是这些歌手的头像、名字和「几首 · 前几个词」，筛选直接放在底上，词浮在一块毛玻璃里；
/// 点词在右边列出它出现在哪些歌，点歌从这个词开始放。
@MainActor
struct CloudView: View {
    @Environment(AppModel.self) private var app
    @AppStorage("cloud.count") private var count = 120
    @AppStorage("cloud.hideFunction") private var hideFunction = true
    @AppStorage("cloud.colorByPos") private var colorByPos = false
    @AppStorage("cloud.font") private var family = "Klee One"
    @AppStorage("cloud.hidden") private var hiddenJSON = "[]"
    @State private var canvasSize = CGSize.zero
    @State private var hovered: String?
    @State private var photos: [NSImage] = []
    /// photos 拼好、虚化好的底图
    @State private var blurredBackdrop: NSImage?
    /// 底图拼的是谁：选了歌手就是选中的人，没选就是曲数最多、有照片的几位（和 Windows 版一样）
    @State private var backdropPeople: [DirectoryEntry] = []
    @State private var colors = ArtworkColors()
    private var model: VisualizationModel { app.visualizations }
    private var hidden: Set<String> { Set((try? JSONDecoder().decode([String].self,from: Data(hiddenJSON.utf8))) ?? []) }
    private var requestKey: String { "\(app.libraryURL?.path ?? "")|\(app.library.version)|\(model.cloudFilter)|\(model.cloudPos)|\(app.scopes.active?.id ?? -1)|\(app.scopes.active?.filter ?? StatsFilter())|\(count)|\(hideFunction)|\(hiddenJSON)" }
    private var chosen: [DirectoryEntry] { model.entries.filter { model.cloudFilter.performerIds.contains($0.id) } }
    /// 没有照片可取色时的高光（Windows 的 DEFAULT_TINT）
    private static let defaultTint = Color(red: 159 / 255, green: 216 / 255, blue: 1)
    private var tint: Color { photos.isEmpty ? Self.defaultTint : colors.highlight }

    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            VStack(alignment: .leading, spacing: 12) {
                header
                primaryFilters
                cloudGlass
                if colorByPos { legend }
                nowSinging
            }
            .frame(minWidth: 320, maxWidth: .infinity, maxHeight: .infinity)
            if let picked = model.picked {
                CloudSongPanel(word: picked, hide: { hide(picked) })
                    .frame(minWidth: 270, idealWidth: 320, maxWidth: 380)
                    .transition(.move(edge: .trailing).combined(with: .opacity))
            }
        }
        .padding(18)
        .animation(.spring(duration: 0.35), value: model.picked?.lemma)
        .background { CloudBackdrop(photos: photos, blurred: blurredBackdrop).ignoresSafeArea() }
        // 底是照片：不管系统深浅色，一律深底亮字（和 Windows 版一样）
        .environment(\.colorScheme, .dark)
        .foregroundStyle(.white)
        .task(id: requestKey) {
            guard let client = app.core else { return }
            await model.load(client,key: "\(app.libraryURL?.path ?? "demo")|\(app.library.version)")
            await model.loadCloud(client,scope: app.scopes.active,count: count,hideFunctionWords: hideFunction,hidden: hidden)
            let pool = chosen.isEmpty ? model.entries.filter { $0.performer > 0 }.sorted { $0.performer > $1.performer } : chosen
            let people = Array(pool.filter { !$0.imagePath.isEmpty }.prefix(5))
            var images: [NSImage] = []
            for p in people {
                if let path = try? await client.resolveMedia(p.imagePath), let image = loadImage(atPath: path) { images.append(image) }
            }
            // 拼图和整屏差不多宽（1.36 倍是 CloudBackdrop 往外多铺的那圈）
            let blurred = await Artwork.blurredCollage(images, radius: 48, saturation: 1.5, displayWidth: 1400 * 1.36)
            if !Task.isCancelled { photos = images; blurredBackdrop = blurred; backdropPeople = people; colors = ArtworkColors(image: images.first) }
        }
        .onAppear { model.cloudPresent = NSApplication.shared.keyWindow?.styleMask.contains(.fullScreen) ?? false }
        .onExitCommand { if !app.showStage { if model.picked != nil { model.picked = nil } else if model.cloudPresent { NSApplication.shared.keyWindow?.toggleFullScreen(nil); model.cloudPresent = false } } }
        .onChange(of: model.error) { _, error in if let error { app.errorMessage = error; model.error = nil } }
    }

    // MARK: - 头部

    private var header: some View {
        let faces = Array((chosen.isEmpty ? backdropPeople : chosen).prefix(3))
        let songs = chosen.isEmpty ? app.tracks.count : Int(chosen.map(\.performer).reduce(0, +))
        return HStack(spacing: 14) {
            HStack(spacing: -12) {
                if faces.isEmpty {
                    Text("全").font(.system(size: 22, weight: .semibold))
                        .frame(width: 52, height: 52)
                        .background(Circle().fill(.white.opacity(0.12)))
                } else {
                    ForEach(faces) { person in
                        CoverImage(path: person.imagePath, size: 52, circle: true, identity: person.name)
                            .overlay(Circle().strokeBorder(.white.opacity(0.35), lineWidth: 2))
                            .shadow(color: .black.opacity(0.35), radius: 10, y: 6)
                    }
                }
            }
            VStack(alignment: .leading, spacing: 3) {
                Text(chosen.isEmpty ? "歌词里的词" : chosen.map(\.name).joined(separator: "、"))
                    .font(.title2.bold()).lineLimit(1)
                Text("\(songs) 首 · 前 \(model.words.count) 个词 · 点词看出现在哪些歌")
                    .font(.caption).monospacedDigit().foregroundStyle(.white.opacity(0.72))
            }
            Spacer(minLength: 12)
            tools
        }
    }

    private var tools: some View {
        HStack(spacing: 10) {
            if !hidden.isEmpty { Button("已隐藏 \(hidden.count) 个 · 恢复") { hiddenJSON = "[]" }.buttonStyle(.borderless) }
            CloudSettings(count: $count,hideFunction: $hideFunction,colorByPos: $colorByPos,family: $family)
            Button { export() } label: { Image(systemName: "square.and.arrow.up") }.buttonStyle(.borderless).help("保存 PNG")
            Button {
                let window = NSApplication.shared.keyWindow
                model.cloudPresent = !(window?.styleMask.contains(.fullScreen) ?? false)
                window?.toggleFullScreen(nil)
            } label: { Image(systemName: "arrow.up.left.and.arrow.down.right") }.buttonStyle(.borderless).help("全屏词云")
        }
    }

    private var primaryFilters: some View {
        @Bindable var model = model
        return HStack(spacing: 8) {
            PerformerPicker(selected: $model.cloudFilter.performerIds)
            SubcorpusPicker()
            Menu(model.cloudPos.isEmpty ? "全部词性" : model.cloudPos.map { posLabels[$0] ?? $0 }.joined(separator: "、")) {
                Button("全部") { model.cloudPos = [] }
                ForEach(["NOUN","PROPN","VERB","ADJ","ADV"],id: \.self) { p in
                    Toggle(posLabels[p] ?? p,isOn: Binding(get: { model.cloudPos.contains(p) },set: { on in
                        if on { model.cloudPos = ["NOUN","PROPN","VERB","ADJ","ADV"].filter { $0 == p || model.cloudPos.contains($0) } }
                        else { model.cloudPos.removeAll { $0 == p }; if model.cloudPos.isEmpty { model.cloudPos = ["NOUN"] } }
                    }))
                }
            }.fixedSize()
            TextField("最早",text: year(\.yearFrom)).frame(width: 56); Text("–"); TextField("最近",text: year(\.yearTo)).frame(width: 56)
            Toggle("仅日文",isOn: Binding(get: { model.cloudFilter.jpOnly },set: { model.cloudFilter.jpOnly = $0 }))
            Spacer(minLength: 0)
        }.textFieldStyle(.roundedBorder).controlSize(.small)
    }

    // MARK: - 云

    private var cloudGlass: some View {
        GeometryReader { geometry in
            let placed = model.placed, tint = tint, colorByPos = colorByPos, font = family, hot = hovered
            WordCloudPainting(placed: placed,family: font,tint: tint,colorByPos: colorByPos,hovered: hot)
                .contentShape(Rectangle())
                .onContinuousHover { phase in
                    switch phase { case .active(let p): hovered = word(at: p)?.lemma; case .ended: hovered = nil }
                }
                .help(hovered.flatMap { lemma in model.words.first { $0.lemma == lemma }.map { "\($0.lemma) · \($0.freq) 次 · \($0.songCount) 首" } } ?? "点一个词，看看出现在哪些歌里")
                .onTapGesture { p in if let w = word(at: p) { model.picked = w } }
                .task(id: "\(geometry.size)|\(model.words)|\(family)") { await layout(geometry.size) }
        }
        .padding(10)
        .background { CloudGlass() }
        .overlay { if model.cloudBusy { ProgressView() } }
        .overlay {
            if !model.cloudBusy, model.words.isEmpty {
                Text("这个范围里没有词。换个歌手、年份或词性试试").foregroundStyle(.white.opacity(0.6))
            }
        }
        .frame(maxHeight: .infinity)
    }

    /// 按词性上色时的图例：每种词性几个词
    private var legend: some View {
        let counts = Dictionary(grouping: model.words, by: { $0.mainPos }).mapValues(\.count)
        let order = ["VERB","NOUN","ADJ","PRON","ADV","INTJ","NUM","PROPN"]
        let shown = order.filter { (counts[$0] ?? 0) > 0 } + counts.keys.filter { !order.contains($0) }.sorted()
        return HStack(spacing: 14) {
            ForEach(shown, id: \.self) { pos in
                HStack(spacing: 5) {
                    Circle().fill(cloudPosColor(pos)).frame(width: 8, height: 8)
                    Text(posLabels[pos] ?? pos)
                    Text("\(counts[pos] ?? 0)").foregroundStyle(.white.opacity(0.55)).monospacedDigit()
                }
            }
            Spacer(minLength: 0)
        }.font(.caption)
    }

    /// 正在唱的那一句当字幕，选中的词标亮
    @ViewBuilder private var nowSinging: some View {
        if let line = app.player.lines.first(where: { $0.utteranceId == app.player.currentUtterance }) {
            VStack(spacing: 4) {
                Text("♪ \(app.player.track?.title ?? "") · \(app.player.track?.artist ?? "")").font(.caption).foregroundStyle(.white.opacity(0.6))
                caption(line.text).font(.title3.weight(.medium))
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 4)
        }
    }

    private func year(_ key: WritableKeyPath<StatsFilter,Int?>) -> Binding<String> {
        Binding(get: { model.cloudFilter[keyPath: key].map(String.init) ?? "" },set: {
            model.cloudFilter[keyPath: key] = Int($0)
            if let a = model.cloudFilter.yearFrom,let b = model.cloudFilter.yearTo,a > b { if key == \StatsFilter.yearFrom { model.cloudFilter.yearTo = a } else { model.cloudFilter.yearFrom = b } }
        })
    }
    private func word(at p: CGPoint) -> CloudWord? { model.placed.first { p.x >= $0.x && p.x <= $0.x+$0.w && p.y >= $0.y && p.y <= $0.y+$0.h }?.word }
    private func caption(_ text: String) -> Text {
        guard let word = model.picked else { return Text(text) }
        let variants = Array(Set(word.surfaces+word.variants)).filter { !$0.isEmpty }.sorted { $0.count > $1.count }
        guard !variants.isEmpty,let regex = try? NSRegularExpression(pattern: variants.map { NSRegularExpression.escapedPattern(for: $0) }.joined(separator: "|")) else { return Text(text) }
        let source = text as NSString; var result = Text(""), cursor = 0
        for match in regex.matches(in: text,range: NSRange(location: 0,length: source.length)) {
            result = result+Text(source.substring(with: NSRange(location: cursor,length: match.range.location-cursor)))+Text(source.substring(with: match.range)).bold().foregroundColor(tint)
            cursor = NSMaxRange(match.range)
        }
        return result+Text(source.substring(from: cursor))
    }
    private func hide(_ word: CloudWord) { let list = Array(hidden.union(word.variants)); hiddenJSON = String(data: (try? JSONEncoder().encode(list)) ?? Data("[]".utf8),encoding: .utf8) ?? "[]"; model.picked = nil }
    private func layout(_ size: CGSize) async {
        canvasSize = size; let words = model.words, family = family
        let task = Task.detached(priority: .userInitiated) { () -> [PlacedWord] in
            let scale = sqrt(size.width*size.height/(900*440))
            let opts = CloudOptions(width: size.width,height: size.height,minFont: floor(min(20,max(12,13*scale))+0.5),maxFont: min(floor(84*scale+0.5),size.width/8,size.height/3),verticalRatio: 0.22) { text,size in
                let font = NSFont(name: family,size: size) ?? NSFont.systemFont(ofSize: size)
                return (text as NSString).size(withAttributes: [.font:font]).width
            }
            return CloudLayout.layoutCloud(words,opts)
        }
        let placed = await withTaskCancellationHandler { await task.value } onCancel: { task.cancel() }
        if !Task.isCancelled { model.placed = placed }
    }
    private func export() {
        guard let client = app.core, canvasSize.width > 0, canvasSize.height > 0, !model.cloudBusy else { return }
        let panel = NSSavePanel(); panel.title = "保存词云"; panel.allowedContentTypes = [.png]; panel.nameFieldStringValue = "wordcloud.png"
        guard panel.runModal() == .OK,let url = panel.url else { return }
        let who = chosen.isEmpty ? "全部歌手" : chosen.prefix(3).map(\.name).joined(separator: "、") + (chosen.count > 3 ? " 等 \(chosen.count) 位" : "")
        let content = VStack(alignment: .leading, spacing: 0) {
            HStack { Text(who).font(.system(size: 22, weight: .semibold)); Spacer(); Text("JPOP Corpus Tool").font(.system(size: 12)).foregroundStyle(.white.opacity(0.6)) }.frame(height: 64, alignment: .top)
            WordCloudPainting(placed: model.placed,family: family,tint: tint,colorByPos: colorByPos,hovered: nil).frame(width: canvasSize.width,height: canvasSize.height)
                .padding(10).background { CloudGlass(material: false) }
        }.padding(28).foregroundStyle(.white).background { CloudBackdrop(photos: photos, blurred: blurredBackdrop) }.environment(\.colorScheme, .dark)
        let renderer = ImageRenderer(content: content); renderer.scale = 2
        guard let image = renderer.cgImage,let png = NSBitmapImageRep(cgImage: image).representation(using: .png,properties: [:]) else { app.errorMessage = "生成图片失败"; return }
        Task { do { try await client.exportPng(path: url.path, bytes: Array(png)) } catch { app.errorMessage = error.localizedDescription } }
    }
}

/// 词浮在上面的那块毛玻璃（Windows 的 .wc-glass）：浅白渐变 + 细白边 + 下方投影。
/// 导出 PNG 时 ImageRenderer 画不出系统材质，只用渐变
private struct CloudGlass: View {
    var material = true
    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 14, style: .continuous)
        ZStack {
            if material { shape.fill(.ultraThinMaterial) }
            shape.fill(LinearGradient(stops: [
                .init(color: .white.opacity(0.14), location: 0),
                .init(color: .white.opacity(0.04), location: 0.45),
                .init(color: .white.opacity(0.07), location: 1),
            ], startPoint: .topLeading, endPoint: .bottomTrailing))
            shape.strokeBorder(.white.opacity(0.18), lineWidth: 1)
        }
        .compositingGroup()
        .shadow(color: .black.opacity(0.35), radius: 25, y: 20)
    }
}

/// 整块的底：歌手照片放大、重度虚化、提饱和铺满（几位就拼几块），没有照片时是蓝紫两团光；
/// 上面再压一层中间亮、四周暗的暗角，字才看得清
@MainActor
private struct CloudBackdrop: View {
    let photos: [NSImage]
    var blurred: NSImage? = nil
    private static let base = Color(red: 0x15 / 255, green: 0x15 / 255, blue: 0x1c / 255)
    private static let shade = Color(red: 10 / 255, green: 10 / 255, blue: 16 / 255)
    var body: some View {
        GeometryReader { geometry in
            let width = geometry.size.width, height = geometry.size.height, reach = max(width, height)
            ZStack {
                Self.base
                if photos.isEmpty {
                    RadialGradient(colors: [Color(red: 76 / 255, green: 194 / 255, blue: 1).opacity(0.45), .clear],
                                   center: UnitPoint(x: 0.3, y: 0.4), startRadius: 0, endRadius: reach * 0.45)
                    RadialGradient(colors: [Color(red: 160 / 255, green: 110 / 255, blue: 1).opacity(0.4), .clear],
                                   center: UnitPoint(x: 0.7, y: 0.6), startRadius: 0, endRadius: reach * 0.45)
                } else if let blurred {
                    Image(nsImage: blurred).resizable()
                        .frame(width: width * 1.36, height: height * 1.36)
                        .frame(width: width, height: height)
                } else {
                    // 虚化图还没算好时的样子（算好前只出现一瞬）
                    // 往外多铺 18%：重度模糊的边缘会发灰
                    let w = width * 1.36, h = height * 1.36
                    ZStack(alignment: .topLeading) {
                        ForEach(Array(photos.prefix(5).enumerated()), id: \.offset) { i, image in
                            let spot = photos.count == 2 ? [(0.0, 0.0), (0.45, 0.2)][i]
                                : [(0.0, 0.0), (0.45, 0.0), (0.0, 0.45), (0.45, 0.45), (0.22, 0.22)][i]
                            let single = photos.count == 1
                            Image(nsImage: image).resizable().scaledToFill()
                                .frame(width: single ? w : w * 0.55, height: single ? h : h * 0.55)
                                .clipped()
                                .offset(x: single ? 0 : spot.0 * w, y: single ? 0 : spot.1 * h)
                        }
                    }
                    .frame(width: w, height: h, alignment: .topLeading)
                    .blur(radius: 48, opaque: true)
                    .saturation(1.5)
                    .frame(width: width, height: height)
                }
                RadialGradient(colors: [Self.shade.opacity(0.15), Self.shade.opacity(0.65)],
                               center: UnitPoint(x: 0.5, y: 0.3), startRadius: 0, endRadius: reach * 0.75)
            }
            .frame(width: width, height: height)
        }
        .clipped()
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}

/// 画词。名次越靠后越淡；前六名用高光色并带一圈光晕，其余白字带一点投影（照片底上也看得清）
struct WordCloudPainting: View {
    let placed: [PlacedWord]; let family: String; let tint: Color; let colorByPos: Bool; let hovered: String?
    var body: some View {
        let items = placed, family = family, tint = tint, byPos = colorByPos, hot = hovered
        Canvas { context, _ in
            for p in items {
                let alpha = 0.5+0.5*pow(1-Double(p.rank)/Double(max(1,items.count)),1.4)
                let top = p.rank < 6
                let color = byPos ? cloudPosColor(p.word.mainPos) : top ? tint : Color.white
                let font: Font = family.isEmpty ? .system(size: p.fontSize) : .custom(family,size: p.fontSize)
                var c = context
                c.opacity = hot == p.word.lemma ? 1 : (byPos ? max(0.62, alpha) : alpha)
                if top { c.addFilter(.shadow(color: color.opacity(0.4), radius: 12)) }
                c.addFilter(.shadow(color: .black.opacity(0.35), radius: 4, y: 1))
                if p.vertical {
                    for (i,ch) in p.word.lemma.unicodeScalars.enumerated() { c.draw(Text(String(ch)).font(font).foregroundColor(color),at: CGPoint(x: p.x+p.w/2,y: p.y+p.fontSize*0.06+p.fontSize*(Double(i)+0.5))) }
                } else { c.draw(Text(p.word.lemma).font(font).foregroundColor(color),at: CGPoint(x: p.x+p.w/2,y: p.y+p.h/2)) }
            }
        }
    }
}
func cloudPosColor(_ pos: String) -> Color {
    let rgb = CloudLayout.posColors[pos] ?? [220,220,228]
    return Color(red: rgb[0]/255,green: rgb[1]/255,blue: rgb[2]/255)
}
