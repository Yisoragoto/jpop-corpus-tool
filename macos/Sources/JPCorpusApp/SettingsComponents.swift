import SwiftUI

struct SettingRow<Control: View>: View {
    let icon: String
    let title: String
    let description: String
    let selectableDescription: Bool
    private let control: () -> Control

    init(icon: String, title: String, description: String = "", selectableDescription: Bool = false,
         @ViewBuilder control: @escaping () -> Control) {
        self.icon = icon
        self.title = title
        self.description = description
        self.selectableDescription = selectableDescription
        self.control = control
    }

    var body: some View {
        // 窄窗口把控件放到下一行，避免歌词面板挤掉设置标题。
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 12) {
                rowLabel
                controls
            }.frame(minWidth: 380)
            VStack(alignment: .leading, spacing: 12) {
                rowLabel
                HStack {
                    Spacer(minLength: 0)
                    controls
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 13)
        .frame(minHeight: 64)
    }

    private var rowLabel: some View {
        HStack(spacing: 12) {
            Image(systemName: icon)
                .font(.system(size: 17))
                .foregroundStyle(.secondary)
                .frame(width: 24)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 3) {
                Text(title).font(.body.weight(.medium))
                if !description.isEmpty {
                    if selectableDescription { detail.textSelection(.enabled) }
                    else { detail }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var controls: some View {
        HStack(spacing: 8, content: control)
            .labelsHidden()
            .fixedSize(horizontal: true, vertical: false)
    }

    private var detail: some View {
        Text(description)
            .font(.caption)
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .truncationMode(.middle)
            .help(description)
    }
}

struct SettingGroup<Content: View>: View {
    let title: String
    private let content: () -> Content

    init(_ title: String, @ViewBuilder content: @escaping () -> Content) {
        self.title = title
        self.content = content
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title)
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(.secondary)
                .padding(.leading, 16)
                .accessibilityAddTraits(.isHeader)
            VStack(spacing: 0, content: content)
                .frame(maxWidth: .infinity)
                .background(Color(nsColor: .controlBackgroundColor).opacity(0.65),
                            in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        }
    }
}

struct SettingDivider: View {
    var body: some View { Divider().padding(.leading, 52) }
}

struct SettingNote: View {
    let text: String
    init(_ text: String) { self.text = text }

    var body: some View {
        Text(text)
            .font(.caption)
            .foregroundStyle(.secondary)
            .textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16)
    }
}

struct SettingSlider: View {
    let title: String
    @Binding var value: Double
    let range: ClosedRange<Double>
    var unit = ""

    var body: some View {
        HStack(spacing: 10) {
            Slider(title, value: $value, in: range, step: 1).frame(width: 130)
            Text("\(Int(value))\(unit)")
                .font(.callout)
                .foregroundStyle(.secondary)
                .monospacedDigit()
                .frame(minWidth: 45, alignment: .trailing)
        }
    }
}
