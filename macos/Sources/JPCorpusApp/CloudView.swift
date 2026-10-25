import AppKit
import JPCore
import JPCorpusModel
import SwiftUI
import UniformTypeIdentifiers

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
    @State private var colors = ArtworkColors()
    private var model: VisualizationModel { app.visualizations }
    private var hidden: Set<String> { Set((try? JSONDecoder().decode([String].self,from: Data(hiddenJSON.utf8))) ?? []) }
    private var requestKey: String { "\(app.libraryURL?.path ?? "")|\(app.library.version)|\(model.cloudFilter)|\(model.cloudPos)|\(app.scopes.active?.id ?? -1)|\(app.scopes.active?.filter ?? StatsFilter())|\(count)|\(hideFunction)|\(hiddenJSON)" }
    var body: some View {
        @Bindable var model = model
        VStack(spacing: 0) {
            filters
            HSplitView {
                VStack(spacing: 0) {
                    HStack { Text(model.cloudFilter.performerIds.isEmpty ? "歌词里的词" : model.entries.filter { model.cloudFilter.performerIds.contains($0.id) }.map(\.name).joined(separator: "、")).font(.title2.bold()); Spacer(); Text("\(model.words.count) 个词").foregroundStyle(.secondary) }.padding(16)
                    GeometryReader { geometry in
                        let placed = model.placed, tint = colors.highlight, colorByPos = colorByPos, font = family, hot = hovered
                        WordCloudPainting(placed: placed,family: font,tint: tint,colorByPos: colorByPos,hovered: hot)
                            .contentShape(Rectangle())
                            .onContinuousHover { phase in
                                switch phase { case .active(let p): hovered = word(at: p)?.lemma; case .ended: hovered = nil }
                            }
                            .help(hovered.flatMap { lemma in model.words.first { $0.lemma == lemma }.map { "\($0.lemma) · \($0.freq) 次 · \($0.songCount) 首" } } ?? "点一个词，看看出现在哪些歌里")
                            .onTapGesture { p in if let w = word(at: p) { model.picked = w } }
                            .task(id: "\(geometry.size)|\(model.words)|\(family)") { await layout(geometry.size) }
                    }
                    if colorByPos { HStack { ForEach(["NOUN","PROPN","VERB","ADJ","ADV"],id: \.self) { p in Text(posLabels[p] ?? p).font(.caption).foregroundStyle(cloudPosColor(p)) } }.padding(8) }
                    if let line = app.player.lines.first(where: { $0.utteranceId == app.player.currentUtterance }) {
                        VStack { Text("♪ \(app.player.track?.title ?? "") · \(app.player.track?.artist ?? "")").font(.caption).foregroundStyle(.secondary); caption(line.text).font(.title3) }.padding(14)
                    }
                }
                .frame(minWidth: 300)
                .background { CloudBackdrop(photos: photos,color: colors.backdrop) }
                .glassCard()
                // Windows 的照片词云在浅色主题下也保持深底亮字。
                .environment(\.colorScheme, .dark)
                .overlay { if model.cloudBusy { ProgressView() } }
                if let picked = model.picked { CloudSongPanel(word: picked,hide: { hide(picked) }).frame(minWidth: 270,idealWidth: 320,maxWidth: 400) }
            }.padding(12)
        }
        .task(id: requestKey) {
            guard let client = app.core else { return }
            await model.load(client,key: "\(app.libraryURL?.path ?? "demo")|\(app.library.version)")
            await model.loadCloud(client,scope: app.scopes.active,count: count,hideFunctionWords: hideFunction,hidden: hidden)
            var images: [NSImage] = []
            let candidates = model.entries.filter { model.cloudFilter.performerIds.isEmpty ? $0.performer > 0 : model.cloudFilter.performerIds.contains($0.id) }.prefix(5)
            for p in candidates { if let path = try? await client.resolveMedia(p.imagePath),let image = loadImage(atPath: path) { images.append(image) } }
            if !Task.isCancelled { photos = images; colors = ArtworkColors(image: images.first) }
        }
        .onAppear { model.cloudPresent = NSApplication.shared.keyWindow?.styleMask.contains(.fullScreen) ?? false }
        .onExitCommand { if !app.showStage { if model.picked != nil { model.picked = nil } else if model.cloudPresent { NSApplication.shared.keyWindow?.toggleFullScreen(nil); model.cloudPresent = false } } }
        .onChange(of: model.error) { _, error in if let error { app.errorMessage = error; model.error = nil } }
    }
    private var filters: some View {
        return ViewThatFits(in: .horizontal) {
            HStack { primaryFilters; options }.padding(10)
            VStack(alignment: .leading) { HStack { primaryFilters }; HStack { options } }.padding(10)
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
        }.textFieldStyle(.roundedBorder).controlSize(.small)
    }
    private var options: some View {
        HStack {
            if !hidden.isEmpty { Button("已隐藏 \(hidden.count) 个 · 恢复") { hiddenJSON = "[]" }.buttonStyle(.borderless) }
            CloudSettings(count: $count,hideFunction: $hideFunction,colorByPos: $colorByPos,family: $family)
            Toggle("仅日文",isOn: Binding(get: { model.cloudFilter.jpOnly },set: { model.cloudFilter.jpOnly = $0 })).controlSize(.small)
            Button { let window = NSApplication.shared.keyWindow; model.cloudPresent = !(window?.styleMask.contains(.fullScreen) ?? false); window?.toggleFullScreen(nil) } label: { Image(systemName: "arrow.up.left.and.arrow.down.right") }.buttonStyle(.borderless).help("全屏词云")
            Button { export() } label: { Image(systemName: "square.and.arrow.up") }.buttonStyle(.borderless).help("导出 PNG")
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
            result = result+Text(source.substring(with: NSRange(location: cursor,length: match.range.location-cursor)))+Text(source.substring(with: match.range)).bold().foregroundColor(colors.highlight)
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
        let chosen = model.entries.filter { model.cloudFilter.performerIds.contains($0.id) }
        let who = chosen.isEmpty ? "全部歌手" : chosen.prefix(3).map(\.name).joined(separator: "、") + (chosen.count > 3 ? " 等 \(chosen.count) 位" : "")
        let content = VStack(alignment: .leading, spacing: 0) {
            HStack { Text(who).font(.system(size: 22, weight: .semibold)); Spacer(); Text("JPOP Corpus Tool").font(.system(size: 12)).foregroundStyle(.secondary) }.frame(height: 64, alignment: .top)
            WordCloudPainting(placed: model.placed,family: family,tint: colors.highlight,colorByPos: colorByPos,hovered: nil).frame(width: canvasSize.width,height: canvasSize.height)
                .background(RoundedRectangle(cornerRadius: 12).fill(.white.opacity(0.07)).padding(-10))
        }.padding(28).foregroundStyle(.white).background { CloudBackdrop(photos: photos,color: colors.backdrop) }.environment(\.colorScheme, .dark)
        let renderer = ImageRenderer(content: content); renderer.scale = 2
        guard let image = renderer.cgImage,let png = NSBitmapImageRep(cgImage: image).representation(using: .png,properties: [:]) else { app.errorMessage = "生成图片失败"; return }
        Task { do { try await client.exportPng(path: url.path, bytes: Array(png)) } catch { app.errorMessage = error.localizedDescription } }
    }
}

@MainActor
private struct CloudBackdrop: View {
    let photos: [NSImage]; let color: Color
    var body: some View {
        GeometryReader { geometry in
            let width = geometry.size.width, height = geometry.size.height
            ZStack(alignment: .topLeading) {
                color
                ForEach(Array(photos.prefix(5).enumerated()),id: \.offset) { i,image in
                    let pair = [(0.0,0.0),(0.5,0.25)], spots = [(0.0,0.0),(0.5,0.0),(0.0,0.5),(0.5,0.5),(0.25,0.25)]
                    let spot = photos.count == 2 ? pair[i] : spots[i]
                    Image(nsImage: image).resizable().scaledToFill()
                        .frame(width: photos.count == 1 ? width : width*0.55,height: photos.count == 1 ? height : height*0.55)
                        .clipped().offset(x: photos.count == 1 ? 0 : spot.0*width,y: photos.count == 1 ? 0 : spot.1*height)
                        .blur(radius: 48).opacity(0.5)
                }
            }
        }.clipped().allowsHitTesting(false)
    }
}

struct WordCloudPainting: View {
    let placed: [PlacedWord]; let family: String; let tint: Color; let colorByPos: Bool; let hovered: String?
    var body: some View {
        let items = placed, family = family, tint = tint, byPos = colorByPos, hot = hovered
        Canvas { context, _ in
            for p in items {
                let alpha = 0.5+0.5*pow(1-Double(p.rank)/Double(max(1,items.count)),1.4)
                let color = byPos ? cloudPosColor(p.word.mainPos) : p.rank < 6 ? tint : Color.white
                let font: Font = family.isEmpty ? .system(size: p.fontSize) : .custom(family,size: p.fontSize)
                var c = context; c.opacity = hot == p.word.lemma ? 1 : alpha
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
