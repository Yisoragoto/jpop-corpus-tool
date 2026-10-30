import JPCore
import SwiftUI

/// 首次启动选语言。两种语言并排写，选之前不知道用户读哪种
@MainActor
struct LanguageChoiceView: View {
    let choose: (String) -> Void

    var body: some View {
        VStack(spacing: 18) {
            Image(systemName: "globe").font(.system(size: 34)).foregroundStyle(.tint)
            Text(verbatim: "选择界面语言 · Choose a language").font(.title2.weight(.semibold))
            Text(verbatim: "之后可以在「设置 → 外观」里更改。\nYou can change this later in Settings → Appearance.")
                .font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center)
            HStack(spacing: 12) {
                Button { choose("zh") } label: { Text(verbatim: "中文").frame(minWidth: 110) }
                Button { choose("en") } label: { Text(verbatim: "English").frame(minWidth: 110) }
                    .keyboardShortcut(.defaultAction)
            }
            .controlSize(.large)
        }
        .padding(32)
        .frame(width: 440)
        .interactiveDismissDisabled()
    }
}
