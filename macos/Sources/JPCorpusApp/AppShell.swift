import JPCorpusModel
import JPCore
import SwiftUI

/// 导航的两种样子：左侧边栏，或者正文上方居中的一条胶囊栏
enum NavStyle: String, CaseIterable {
    case sidebar, top

    static let key = "nav.style"
}

/// 主窗口的骨架：导航 + 正文。
///
/// 不用 NavigationSplitView：两种导航之间要能平滑地变过去。正文一直在同一个位置（只改左边距 / 上边距），
/// 页面不会因为换导航而重建；导航项在侧栏和胶囊栏里用同一个 matchedGeometryEffect 的 id，
/// 切换时每一项从竖排飞到横排，选中的高亮块也跟着滑。
@MainActor
struct AppShell<Detail: View>: View {
    @Environment(AppModel.self) private var app
    @AppStorage(NavStyle.key) private var stored: NavStyle = .sidebar
    /// 真正拿来排版的样式：设置页改 AppStorage 时也要带动画，所以另存一份，在动画事务里改它
    @State private var style: NavStyle = NavStyle(rawValue: UserDefaults.standard.string(forKey: NavStyle.key) ?? "") ?? .sidebar
    @State private var width: CGFloat = 1200
    @Namespace private var nav
    private let detail: Detail

    init(@ViewBuilder detail: () -> Detail) {
        self.detail = detail()
    }

    static var railWidth: CGFloat { 192 }
    static var barHeight: CGFloat { 54 }
    static var motion: Animation { .spring(response: 0.46, dampingFraction: 0.86) }

    /// 侧栏分两组，和 Windows 侧栏的顺序一致；设置单独放在最下面（顶栏里放最右边）
    struct NavGroup: Identifiable {
        let title: String
        let items: [SidebarItem]
        var id: String { title }
    }

    static var groups: [NavGroup] {
        [NavGroup(title: "媒体库", items: [.home, .library, .search, .analysis]),
         NavGroup(title: "工具", items: [.importer, .scrape, .dictionaries, .anki])]
    }

    static var allItems: [SidebarItem] { groups.flatMap { $0.items } }

    var body: some View {
        detail
            .padding(.leading, style == .sidebar ? Self.railWidth : 0)
            .padding(.top, style == .top ? Self.barHeight : 0)
            .overlay(alignment: .topLeading) {
                if style == .sidebar {
                    rail
                } else {
                    topBar
                }
            }
            .background {
                GeometryReader { proxy in
                    Color.clear
                        .onAppear { width = proxy.size.width }
                        .onChange(of: proxy.size.width) { _, value in width = value }
                }
            }
            .onChange(of: stored) { _, value in
                guard value != style else { return }
                withAnimation(Self.motion) { style = value }
            }
            .toolbar {
                ToolbarItem(placement: .navigation) {
                    Button {
                        stored = style == .sidebar ? .top : .sidebar
                    } label: {
                        Label("切换导航样式", systemImage: style == .sidebar ? "rectangle.topthird.inset.filled" : "sidebar.left")
                    }
                    .help(style == .sidebar ? "改成顶部导航栏" : "改成侧边栏")
                }
            }
    }

    private func select(_ item: SidebarItem) {
        withAnimation(.snappy(duration: 0.28)) { app.section = item }
    }

    // MARK: 侧边栏

    private var rail: some View {
        VStack(alignment: .leading, spacing: 2) {
            ForEach(Self.groups) { group in
                Text(group.title)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 10)
                    .padding(.top, 14)
                    .padding(.bottom, 4)
                    .transition(.opacity)
                ForEach(group.items) { item in railItem(item) }
            }
            Spacer(minLength: 12)
            railItem(.settings)
            Button { app.showLibraryPicker = true } label: {
                LibraryBadge()
            }
            .buttonStyle(.plain)
            .help("打开其他语料库（⌘O）")
            .transition(.opacity)
        }
        .padding(.horizontal, 10)
        .padding(.bottom, 8)
        .frame(width: Self.railWidth, alignment: .leading)
        .frame(maxHeight: .infinity, alignment: .top)
    }

    private func railItem(_ item: SidebarItem) -> some View {
        let selected = app.section == item
        return Button { select(item) } label: {
            HStack(spacing: 9) {
                Image(systemName: item.symbol)
                    .font(.system(size: 14, weight: .medium))
                    .frame(width: 20)
                Text(item.title)
                    .lineLimit(1)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 10)
            .frame(height: 30)
            .foregroundStyle(selected ? Color.accentColor : Color.primary)
            .background {
                if selected {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(Color.accentColor.opacity(0.16))
                        .matchedGeometryEffect(id: "selection", in: nav)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .matchedGeometryEffect(id: item, in: nav)
    }

    // MARK: 顶部胶囊栏

    /// 窗口窄时只留图标（悬停看名字），免得胶囊栏挤出窗口
    private var showsLabels: Bool {
        let needed = Self.allItems.reduce(CGFloat(140)) { sum, item in
            sum + 44 + CGFloat(L10n.t(item.title).count) * (L10n.isEnglish ? 7.5 : 13)
        }
        return width >= needed
    }

    private var topBar: some View {
        HStack(spacing: 10) {
            Spacer(minLength: 0)
            HStack(spacing: 2) {
                ForEach(Self.allItems) { item in barItem(item, label: showsLabels) }
            }
            .padding(4)
            .background(.regularMaterial, in: Capsule())
            .overlay(Capsule().strokeBorder(Color.primary.opacity(0.08)))
            .shadow(color: .black.opacity(0.12), radius: 8, y: 2)
            Spacer(minLength: 0)
        }
        .overlay(alignment: .trailing) {
            barItem(.settings, label: false)
                .padding(4)
                .background(.regularMaterial, in: Capsule())
                .overlay(Capsule().strokeBorder(Color.primary.opacity(0.08)))
                .padding(.trailing, 14)
        }
        .padding(.horizontal, 14)
        .frame(height: Self.barHeight)
        .frame(maxWidth: .infinity)
    }

    private func barItem(_ item: SidebarItem, label: Bool) -> some View {
        let selected = app.section == item
        return Button { select(item) } label: {
            HStack(spacing: 6) {
                Image(systemName: item.symbol)
                    .font(.system(size: 13, weight: .medium))
                if label {
                    Text(item.title)
                        .lineLimit(1)
                        .fixedSize()
                }
            }
            .padding(.horizontal, label ? 12 : 9)
            .frame(height: 32)
            .foregroundStyle(selected ? Color.accentColor : Color.primary)
            .background {
                if selected {
                    Capsule()
                        .fill(Color.accentColor.opacity(0.18))
                        .matchedGeometryEffect(id: "selection", in: nav)
                }
            }
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .help(item.title)
        .matchedGeometryEffect(id: item, in: nav)
    }
}

/// 侧栏底部：当前打开的是哪个库
@MainActor
struct LibraryBadge: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        HStack(spacing: 6) {
            VStack(alignment: .leading, spacing: 2) {
                Text(app.isDemo ? "示例语料库" : (app.libraryURL?.lastPathComponent ?? ""))
                    .font(.caption.weight(.medium))
                    .lineLimit(1)
                Text("\(app.tracks.count) 首")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
            Image(systemName: "arrow.left.arrow.right")
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(10)
        .contentShape(Rectangle())
    }
}
