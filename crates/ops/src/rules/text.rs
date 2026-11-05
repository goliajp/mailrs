//! What each alert says, in English and Chinese: a title, what it means,
//! and the first thing to do.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Text {
    pub title_en: &'static str,
    pub title_zh: &'static str,
    pub meaning_en: &'static str,
    pub meaning_zh: &'static str,
    pub step_en: &'static str,
    pub step_zh: &'static str,
}

pub const QUARANTINED: Text = Text {
    title_en: "Mail was quarantined",
    title_zh: "有信被隔离",
    meaning_en: "A spool file stopped the core three times in a row and was set aside so other mail keeps arriving. It has not been delivered.",
    meaning_zh: "有一封信连续三次让核心进程在投递时退出，已被挪到隔离区，其他信照常投递。这封信还没有投递。",
    step_en: "Read the quarantine record (envelope only). Confirm the build that crashed on it is fixed, then move the file back to incoming/new/ by hand. Do not retry it on an unfixed build.",
    step_zh: "看隔离记录（只有信封信息）。确认引起崩溃的版本已修好，再由人把文件挪回 incoming/new/。不要在没修好的版本上重投。",
};

pub const CORE_MEMORY: Text = Text {
    title_en: "Core memory is high",
    title_zh: "核心内存高",
    meaning_en: "fastcore uses 85% or more of its container memory limit.",
    meaning_zh: "fastcore 用掉了容器内存上限的 85% 以上。",
    step_en: "Check whether a re-judge or backfill is running. If it keeps rising, plan a restart in a quiet hour and review mem_limit.",
    step_zh: "看是不是在跑重判或回填；持续上涨时准备在安静时段重启，并检查 mem_limit。",
};

pub const KEVY_MEMORY: Text = Text {
    title_en: "kevy-server memory is high",
    title_zh: "kevy-server 内存高",
    meaning_en: "kevy-server's resident memory is 85% or more of MAILRS_KEVY_SERVER_MEMORY_LIMIT.",
    meaning_zh: "kevy-server 的常驻内存达到 MAILRS_KEVY_SERVER_MEMORY_LIMIT 的 85% 以上。",
    step_en: "Find which key family is growing. Do not restart kevy-server on a whim: read the BGREWRITEAOF note above it in the compose file first.",
    step_zh: "看哪一族键在涨；不要直接重启 kevy-server（先读 compose 里它上方关于 BGREWRITEAOF 的注释）。",
};

pub const NO_QUOTA: Text = Text {
    title_en: "Accounts without a quota",
    title_zh: "有账号没设限额",
    meaning_en: "Some accounts have no storage limit, so nothing stops one of them from filling the data volume.",
    meaning_zh: "有账号没有设存储限额，其中任何一个都可能把数据卷写满。",
    step_en: "Set a limit on these accounts.",
    step_zh: "给这些账号设限额。",
};

pub const QUOTA_80: Text = Text {
    title_en: "Accounts over 80% of their quota",
    title_zh: "账号用量超过 80%",
    meaning_en: "At least one account has used 80% or more of its storage limit; mail to it will be refused once it is full.",
    meaning_zh: "至少有一个账号用掉了限额的 80% 以上；满了以后发给它的信会被拒收。",
    step_en: "Tell the user to clean up, or raise the limit.",
    step_zh: "通知用户清理，或者调高限额。",
};

pub const NEW_DOMAINS: Text = Text {
    title_en: "Burst of newly seen sender domains",
    title_zh: "新域名突增",
    meaning_en: "More sender domains than usual were seen for the first time in the last 24 hours.",
    meaning_zh: "过去 24 小时里第一次见到的发信域名比平时多。",
    step_en: "Check for a batch of look-alike domains.",
    step_zh: "看是否有成批的仿冒域名。",
};

pub const VOLUME: Text = Text {
    title_en: "Data volume is running out of space",
    title_zh: "数据卷剩余空间少",
    meaning_en: "The mailrs-data volume has less than 10 GB free (critical under 2 GB).",
    meaning_zh: "mailrs-data 卷剩余空间不到 10 GB（不到 2 GB 为 critical）。",
    step_en: "Find whether maildir, kevy or the spool is growing. Before the space runs out the receiver should start refusing mail temporarily.",
    step_zh: "看 maildir、kevy、spool 哪个在涨；空间耗尽前 receiver 应开始临时拒收。",
};

pub const SPOOL_STALLED: Text = Text {
    title_en: "The spool is not moving",
    title_zh: "spool 不再推进",
    meaning_en: "The oldest waiting spool file is over 5 minutes old and the drain has delivered nothing for 5 minutes. No new mail reaches any mailbox.",
    meaning_zh: "最老的待投信超过 5 分钟，而且 5 分钟里一封都没投出去。所有新信都进不了信箱。",
    step_en: "Look in fastcore's log for the file the drain stopped at, and whether the quarantine count is rising.",
    step_zh: "看 fastcore 日志里 drain 停在哪封信、隔离数是否在涨。",
};

pub const SET_ASIDE: Text = Text {
    title_en: "Work was set aside",
    title_zh: "有被搁置的工作",
    meaning_en: "A background loop gave up on items after the process died on them each time.",
    meaning_zh: "后台循环在某些条目上每次都让进程退出，已经放弃处理它们。",
    step_en: "Follow the notes of the feature named in the object (attachment text, fraud backfill or the sender).",
    step_zh: "按对象里写的功能（附件文字、诈骗回填或发信）的说明处理。",
};

pub const PARSING: Text = Text {
    title_en: "Parse failures are rising",
    title_zh: "解析失败增多",
    meaning_en: "A process's parse workers failed on 20 or more messages in an hour, or a worker has been held back for over 5 minutes after dying repeatedly.",
    meaning_zh: "某个进程的解析 worker 一小时内失败 20 次以上，或者有 worker 因为反复死掉被暂停超过 5 分钟。",
    step_en: "See in `failures` which parser and which way it died. If one message keeps failing, find that message.",
    step_zh: "看 failures 里是哪个解析器、哪种死法；同一封信反复失败时找出这封信。",
};

pub const CRASH_LOOP: Text = Text {
    title_en: "A process is restarting in a loop",
    title_zh: "进程循环崩溃",
    meaning_en: "The process started 3 or more times in 15 minutes (counting starts only its counter shows), or it started and has not said it is ready.",
    meaning_zh: "这个进程 15 分钟内启动了 3 次以上（含只从启动计数补出来的启动），或者启动后一直没有就绪。",
    step_en: "Read its last log lines and any panic line. If it is running out of memory, mem_limit is deployment configuration and a person changes it. Do not add restarts on top.",
    step_zh: "看这个进程最后的日志行和 panic 行；内存超限时改 mem_limit 是部署配置，要人改。不要再叠加重启。",
};

pub const DAMAGED: Text = Text {
    title_en: "The core's store came up damaged",
    title_zh: "内嵌库启动报告损坏",
    meaning_en: "fastcore's boot replay reported DAMAGED: it is serving fewer keys than its files held.",
    meaning_zh: "fastcore 启动 replay 报告 DAMAGED：它提供的键比文件里的少。",
    step_en: "Do NOT restart it. Recover by hand following the kevy AOF damage procedure.",
    step_zh: "不要重启。按 kevy AOF 损坏的恢复步骤由人处理。",
};

pub const VERSIONS: Text = Text {
    title_en: "Processes run different builds",
    title_zh: "各进程版本不一致",
    meaning_en: "Outside an upgrade, at least one process reports a version or commit different from the others.",
    meaning_zh: "不在升级过程中，而有进程报告的版本或 commit 和其他进程不同。",
    step_en: "Check the binary each container actually runs. Image tags are not proof (2026-07-15).",
    step_zh: "核对每个容器实际跑的二进制；镜像 tag 不可信（2026-07-15 事故）。",
};

pub const OUTBOUND_STUCK: Text = Text {
    title_en: "Outbound mail is stuck",
    title_zh: "出站卡住",
    meaning_en: "Jobs are waiting and the sender has not taken one for 10 minutes, or the oldest waiting job is over an hour old.",
    meaning_zh: "有任务在等，而 sender 10 分钟没取过任务；或者最老的待发任务超过 1 小时。",
    step_en: "Check whether the sender is alive and whether every job is in backoff; read the remote refusals.",
    step_zh: "看 sender 是否活着、是否所有任务都在退避；看对方拒收的回复。",
};

pub const CHANNEL: Text = Text {
    title_en: "A notification channel is failing",
    title_zh: "通知渠道失败",
    meaning_en: "Three sends in a row on this channel failed; this notice comes through the other channels.",
    meaning_zh: "这个渠道连续 3 次发送失败；这条通知经其他渠道发出。",
    step_en: "Check the channel's configuration and the service it sends to.",
    step_zh: "检查这个渠道的配置和对方服务。",
};

pub const UNWATCHED: Text = Text {
    title_en: "Nothing outside watches the ops daemon",
    title_zh: "运维进程没有外部看护",
    meaning_en: "MAILRS_OPS_HEARTBEAT_URL is not set, so if this daemon or the whole host stops, nobody is told.",
    meaning_zh: "没有配 MAILRS_OPS_HEARTBEAT_URL；这个守护进程或者整台机器停了，没有人会知道。",
    step_en: "Configure an external \"alert when silent\" check URL.",
    step_zh: "配一个外部的「到时间没收到就报警」的检查地址。",
};

pub const MAINTENANCE_LONG: Text = Text {
    title_en: "Maintenance has been on too long",
    title_zh: "维护状态开得太久",
    meaning_en: "One stretch of maintenance has been open for more than the configured hours. While it is on, notifications are held back.",
    meaning_zh: "一次维护状态累计开着超过了设定的小时数。维护期间通知都被压住。",
    step_en: "Confirm someone is still working on it; if not, end maintenance.",
    step_zh: "确认还有没有人在处理；没有就结束维护状态。",
};

pub const SOURCE: Text = Text {
    title_en: "Cannot read a source",
    title_zh: "读不到一个来源",
    meaning_en: "The daemon has not been able to read this source for the unknown wait. Its rules are unknown, not ok.",
    meaning_zh: "守护进程在等待时间里一直读不到这个来源；它下面的规则是「未知」，不是正常。",
    step_en: "Check whether the process is running and reachable from the ops container; for a process that pushes reports, check its MAILRS_OPS_URL and key.",
    step_zh: "看这个进程是否在运行、从 ops 容器能不能连到；对推报告的进程，查它的 MAILRS_OPS_URL 和报告密钥。",
};

pub const FIELD: Text = Text {
    title_en: "Cannot read a value",
    title_zh: "读不到一个值",
    meaning_en: "The source answers, but this rule's value has been missing for the unknown wait.",
    meaning_zh: "来源有回答，但这条规则要的值在等待时间里一直读不到。",
    step_en: "Check the field in the source's health answer and what it depends on.",
    step_zh: "查来源健康信息里的这个字段，以及它依赖的东西。",
};

pub const KEVY_DOWN: Text = Text {
    title_en: "kevy-server is unavailable",
    title_zh: "kevy-server 不可用",
    meaning_en: "webapi cannot reach kevy-server and no process has completed an operation on it recently. Sessions, the outbound queue and side state are affected; alerts that read through it are filed under this one.",
    meaning_zh: "webapi 连不上 kevy-server，最近也没有进程在它上面完成过操作。会话、出站队列和各种旁路状态都受影响；经它读到的告警归到这一条下面。",
    step_en: "Check the kevy-server container and its log. Do not restart it before reading the BGREWRITEAOF note in the compose file.",
    step_zh: "看 kevy-server 容器和它的日志。读过 compose 里关于 BGREWRITEAOF 的注释之前不要重启它。",
};

pub const NO_REPORTS: Text = Text {
    title_en: "No process reports arrive",
    title_zh: "收不到任何进程的报告",
    meaning_en: "None of the processes that push reports has been heard from. Most likely the daemon's own network or report port is the problem, not every process at once.",
    meaning_zh: "所有推报告的进程都没有消息。多半是守护进程这一侧的网络或接收端口出了问题，而不是所有进程同时出了事。",
    step_en: "Check the ops container's network and that MAILRS_OPS_BIND is reachable from the other containers.",
    step_zh: "查 ops 容器的网络，以及别的容器能不能连到 MAILRS_OPS_BIND。",
};
