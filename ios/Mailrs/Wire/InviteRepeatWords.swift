import Foundation

extension InviteRepeat {
    /// The words of a repeat line in one language.
    ///
    /// Chosen by the formatting locale rather than looked up in the string
    /// catalogue, so the language of the words and the language of the
    /// dates and weekday names inside them are always the same one.
    struct Words: Sendable {
        let daily: String
        let dailyN: @Sendable (Int) -> String
        let weekly: @Sendable (String) -> String
        let weeklyN: @Sendable (Int, String) -> String
        let monthly: String
        let monthlyDay: @Sendable (Int) -> String
        let yearly: String
        let other: String
        let until: @Sendable (String) -> String
        let count: @Sendable (Int) -> String
        let next: @Sendable (String) -> String
        let organiserTime: String
        let ended: String
        let daySeparator: String
        /// Weekdays by their short name (周四), as Chinese says them.
        let shortDays: Bool
    }

    static func words(_ locale: Locale) -> Words {
        switch locale.language.languageCode?.identifier {
        case "zh": return chinese
        case "ja": return japanese
        default: return english
        }
    }

    private static let english = Words(
        daily: "Every day",
        dailyN: { "Every \($0) days" },
        weekly: { "Weekly on \($0)" },
        weeklyN: { "Every \($0) weeks on \($1)" },
        monthly: "Every month",
        monthlyDay: { "Monthly on day \($0)" },
        yearly: "Every year",
        other: "Repeats",
        until: { "until \($0)" },
        count: { "\($0) times" },
        next: { "Next: \($0)" },
        organiserTime: " (organizer’s time)",
        ended: "the series has ended",
        daySeparator: ", ",
        shortDays: false
    )

    private static let chinese = Words(
        daily: "每天",
        dailyN: { "每 \($0) 天" },
        weekly: { "每\($0)" },
        weeklyN: { "每 \($0) 周的\($1)" },
        monthly: "每月",
        monthlyDay: { "每月 \($0) 日" },
        yearly: "每年",
        other: "重复",
        until: { "到 \($0) 为止" },
        count: { "共 \($0) 次" },
        next: { "下一次：\($0)" },
        organiserTime: "（按组织者的时间）",
        ended: "已全部结束",
        daySeparator: "、",
        shortDays: true
    )

    private static let japanese = Words(
        daily: "毎日",
        dailyN: { "\($0) 日ごと" },
        weekly: { "毎週\($0)" },
        weeklyN: { "\($0) 週間ごとの\($1)" },
        monthly: "毎月",
        monthlyDay: { "毎月 \($0) 日" },
        yearly: "毎年",
        other: "繰り返し",
        until: { "\($0) まで" },
        count: { "全 \($0) 回" },
        next: { "次回：\($0)" },
        organiserTime: "（主催者の時間）",
        ended: "すべて終了しました",
        daySeparator: "・",
        shortDays: false
    )
}
