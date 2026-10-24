import AppKit
import JPCore
import SwiftUI

/// One native text button per actual source unit. Masking expresses progress
/// only inside that unit's own interval, never between units or by character.
@MainActor
struct TimedLyricText: View {
    let text: String
    let units: [LyricTimingUnit]
    let position: Double
    var size: CGFloat = 19
    let seek: (LyricTimingUnit) -> Void

    var body: some View {
        LyricUnitLayout {
            ForEach(units) { unit in
                let label = unit.text(in: text) ?? ""
                Button { seek(unit) } label: {
                    Text(label)
                        .foregroundStyle(.secondary)
                        .overlay(alignment: .leading) {
                            Text(label)
                                .foregroundStyle(Color.accentColor)
                                .mask {
                                    GeometryReader { geometry in
                                        Rectangle()
                                            .frame(width: geometry.size.width * CGFloat(unit.progress(at: position)))
                                            .frame(maxWidth: .infinity, alignment: .leading)
                                    }
                                }
                                .allowsHitTesting(false)
                                .accessibilityHidden(true)
                        }
                        .fixedSize(horizontal: false, vertical: true)
                }
                .buttonStyle(.plain)
                .help("从「\(label)」的真实起点播放")
                .accessibilityLabel("从「\(label)」开始播放")
            }
        }
        .font(.system(size: size, weight: .bold))
        .animation(nil, value: position)
    }
}

/// Wrap at actual unit boundaries without inserting whitespace into Japanese.
private struct LyricUnitLayout: Layout {
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        arrangement(width: proposal.width, subviews: subviews).size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let result = arrangement(width: bounds.width, subviews: subviews)
        for (index, subview) in subviews.enumerated() {
            let frame = result.frames[index]
            subview.place(
                at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY),
                anchor: .topLeading,
                proposal: ProposedViewSize(width: frame.width, height: frame.height)
            )
        }
    }

    private func arrangement(width: CGFloat?, subviews: Subviews) -> (size: CGSize, frames: [CGRect]) {
        let limit = width ?? .infinity
        var x: CGFloat = 0
        var y: CGFloat = 0
        var rowHeight: CGFloat = 0
        var usedWidth: CGFloat = 0
        var frames: [CGRect] = []
        for subview in subviews {
            let size = subview.sizeThatFits(ProposedViewSize(width: width, height: nil))
            if x > 0, x + size.width > limit {
                x = 0
                y += rowHeight + 5
                rowHeight = 0
            }
            frames.append(CGRect(origin: CGPoint(x: x, y: y), size: size))
            x += size.width
            rowHeight = max(rowHeight, size.height)
            usedWidth = max(usedWidth, x)
        }
        return (CGSize(width: width ?? usedWidth, height: y + rowHeight), frames)
    }
}

/// Selection remains native text selection, separate from the unit seek buttons.
/// Lookup uses exactly the selected substring, not a reconstructed token range.
@MainActor
struct LyricSelectionView: View {
    let text: String
    let lookup: (String) -> Void
    @State private var selected = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("选择原文查词").font(.headline)
            SelectableLyricText(text: text, selection: $selected)
                .frame(height: 110)
            HStack {
                Button("复制整行") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(text, forType: .string)
                }
                Spacer()
                Button("查选中的文字") { lookup(selected) }
                    .disabled(selected.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(14)
        .frame(width: 360)
    }
}

private struct SelectableLyricText: NSViewRepresentable {
    let text: String
    @Binding var selection: String

    func makeCoordinator() -> Coordinator { Coordinator(selection: $selection) }

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        let view = NSTextView()
        view.isEditable = false
        view.isSelectable = true
        view.drawsBackground = false
        view.font = .systemFont(ofSize: 19, weight: .bold)
        view.textColor = .labelColor
        view.textContainerInset = NSSize(width: 5, height: 5)
        view.isVerticallyResizable = true
        view.isHorizontallyResizable = false
        view.autoresizingMask = [.width]
        view.textContainer?.widthTracksTextView = true
        view.delegate = context.coordinator
        view.string = text
        scroll.documentView = view
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        context.coordinator.selection = $selection
        if let view = scroll.documentView as? NSTextView, view.string != text {
            view.string = text
        }
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var selection: Binding<String>
        init(selection: Binding<String>) { self.selection = selection }

        func textViewDidChangeSelection(_ notification: Notification) {
            guard let view = notification.object as? NSTextView else { return }
            let range = view.selectedRange()
            let text = view.string as NSString
            guard range.location != NSNotFound, NSMaxRange(range) <= text.length else { return }
            selection.wrappedValue = text.substring(with: range)
        }
    }
}
