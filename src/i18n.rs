//! UI language and per-page help.
//!
//! The default language is detected from `LC_ALL`/`LC_MESSAGES`/`LANG`; it can
//! be overridden with `--lang` or `MTOP_LANG` and toggled at runtime with `l`.
//! Column abbreviations (PID, RSS, PSS, USS, PSI, OOM) stay in English: they
//! are the kernel/industry names and the help text explains them in both
//! languages.

use std::env;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Zh,
}

impl Lang {
    pub const NAMES: &'static str = "auto, en, zh";

    pub fn detect() -> Self {
        for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(value) = env::var(key)
                && !value.is_empty()
            {
                return Self::from_locale(&value);
            }
        }
        Self::En
    }

    fn from_locale(value: &str) -> Self {
        if value.to_ascii_lowercase().starts_with("zh") {
            Self::Zh
        } else {
            Self::En
        }
    }

    /// Parse a `--lang` value. `auto` performs locale detection; full locale
    /// forms such as `zh_CN.UTF-8` are accepted too.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.to_ascii_lowercase();
        match value.as_str() {
            "auto" => Some(Self::detect()),
            "en" => Some(Self::En),
            "zh" => Some(Self::Zh),
            other if other.starts_with("en_") => Some(Self::En),
            other if other.starts_with("zh_") => Some(Self::Zh),
            _ => None,
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::En => Self::Zh,
            Self::Zh => Self::En,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpPage {
    Procs,
    Cgroups,
    Apps,
    Detail,
    CgroupDrill,
}

pub struct Section {
    pub heading: &'static str,
    pub body: &'static [&'static str],
}

pub struct Doc {
    pub title: &'static str,
    pub sections: &'static [Section],
}

/// Translation helpers. `pick` is for one-off labels; the methods build the
/// dynamic titles/footers so word order can differ between languages.
#[derive(Clone, Copy)]
pub struct T(pub Lang);

impl T {
    pub fn is_zh(&self) -> bool {
        matches!(self.0, Lang::Zh)
    }

    pub fn pick(&self, en: &'static str, zh: &'static str) -> &'static str {
        if self.is_zh() { zh } else { en }
    }

    pub fn procs_title(&self, sort: &str, dir: &str, filter: &str) -> String {
        if self.is_zh() {
            format!(" 进程  排序 {sort} {dir}  过滤 {filter} ")
        } else {
            format!(" procs  sort {sort} {dir}  filter {filter} ")
        }
    }

    pub fn app_filter(&self, name: &str) -> String {
        if self.is_zh() {
            format!("  应用: {name} (Esc 清除) ")
        } else {
            format!("  app: {name} (Esc clears) ")
        }
    }

    pub fn ranking_incomplete(&self, covered: usize, eligible: usize) -> String {
        if self.is_zh() {
            format!(" PSS {covered}/{eligible} · 排名不完整 ")
        } else {
            format!(" pss {covered}/{eligible} · ranking incomplete ")
        }
    }

    pub fn cgroups_title(
        &self,
        sort: &str,
        dir: &str,
        filter: &str,
        shown: usize,
        total: usize,
    ) -> String {
        if self.is_zh() {
            format!(" cgroups  排序 {sort} {dir}  过滤 {filter}  ({shown} / {total}) ")
        } else {
            format!(" cgroups  sort {sort} {dir}  filter {filter}  ({shown} of {total}) ")
        }
    }

    pub fn oom_recent(&self, recent: u64) -> String {
        if self.is_zh() {
            format!("  距上次采样 OOM +{recent} ")
        } else {
            format!("  oom +{recent} since last sample ")
        }
    }

    pub fn apps_title(
        &self,
        metric: &str,
        hint: &str,
        filter: &str,
        groups: usize,
        size: &str,
    ) -> String {
        if self.is_zh() {
            format!(" 应用  指标 {metric}{hint}  过滤 {filter}  ({groups} 组 · {size}) ")
        } else {
            format!(" apps  metric {metric}{hint}  filter {filter}  ({groups} groups · {size}) ")
        }
    }

    pub fn detail_title(&self, filter: Option<&str>) -> String {
        match (self.is_zh(), filter) {
            (false, None) => " vm mappings by PSS  (Enter expand, Esc back, f filter) ".into(),
            (false, Some(filter)) => format!(" vm mappings by PSS  filter {filter} "),
            (true, None) => " 按 PSS 排序的映射  (Enter 展开, Esc 返回, f 过滤) ".into(),
            (true, Some(filter)) => format!(" 按 PSS 排序的映射  过滤 {filter} "),
        }
    }

    pub fn drill_title(&self) -> &'static str {
        self.pick(
            " procs in cgroup subtree  (Esc back) ",
            " cgroup 子树内的进程  (Esc 返回) ",
        )
    }

    pub fn empty_apps(&self) -> &'static str {
        self.pick(
            "no app groups (no PSS data yet)",
            "暂无应用分组（还没有 PSS 数据）",
        )
    }

    pub fn sampling(&self) -> &'static str {
        self.pick("sampling /proc…", "正在采样 /proc…")
    }

    pub fn waiting_sampler(&self) -> &'static str {
        self.pick("waiting for sampler…", "等待采样…")
    }

    pub fn vm_unavailable(&self) -> &'static str {
        self.pick("vm mappings unavailable", "无法读取映射")
    }

    pub fn cgroup_unavailable(&self) -> &'static str {
        self.pick(
            "cgroup v2 memory controller not available\n\n                 (needs /sys/fs/cgroup/memory.current; v1 or a restricted hierarchy shows nothing)",
            "cgroup v2 内存控制器不可用\n\n                 （需要 /sys/fs/cgroup/memory.current；cgroup v1 或受限层级不会显示数据）",
        )
    }

    pub fn drill_summary(
        &self,
        current: &str,
        members: usize,
        limit: &str,
        psi_some: f32,
        psi_full: f32,
    ) -> String {
        if self.is_zh() {
            format!(
                " 当前 {current} · 子树内 {members} 个进程 · 上限 {limit} · PSI {psi_some:.1}/{psi_full:.1}% "
            )
        } else {
            format!(
                " current {current} · {members} procs in subtree · limit {limit} · psi {psi_some:.1}/{psi_full:.1}% "
            )
        }
    }

    pub fn oom_line(&self, total: u64, window: u64, scans: usize) -> String {
        if self.is_zh() {
            format!("  {total} 累计 · 最近 {scans} 次扫描 +{window}")
        } else {
            format!("  {total} total · +{window} in last {scans} scans")
        }
    }

    pub fn vma_summary(&self, vmas: usize, groups: usize, parsed_ms: f32, cmdline: &str) -> String {
        if self.is_zh() {
            format!(" {vmas} 个 VMA，{groups} 组 · 解析 {parsed_ms:.0}ms · {cmdline}")
        } else {
            format!(" {vmas} VMAs in {groups} groups · parsed {parsed_ms:.0}ms · {cmdline}")
        }
    }

    pub fn editing(&self, detail: bool) -> (&'static str, &'static str) {
        match (self.is_zh(), detail) {
            (false, false) => (" filter: ", "   Enter apply · Esc cancel"),
            (false, true) => (" vma filter: ", "   Enter apply · Esc cancel"),
            (true, false) => (" 过滤: ", "   Enter 应用 · Esc 取消"),
            (true, true) => (" VMA 过滤: ", "   Enter 应用 · Esc 取消"),
        }
    }

    pub fn theme_line(&self, theme: &str) -> String {
        if self.is_zh() {
            format!(" 主题: {theme} · t 切换")
        } else {
            format!(" theme: {theme} · t switch")
        }
    }

    pub fn help_footer(&self) -> &'static str {
        self.pick(
            " Esc/? close · j/k scroll · PgUp/PgDn · g/G · l language ",
            " Esc/? 关闭 · j/k 滚动 · PgUp/PgDn · g/G · l 语言 ",
        )
    }

    pub fn help_title(&self, page: HelpPage) -> String {
        format!("{} · {}", self.pick("Help", "帮助"), self.doc(page).title)
    }

    pub fn doc(&self, page: HelpPage) -> &'static Doc {
        match (self.0, page) {
            (Lang::En, HelpPage::Procs) => &PROCS_EN,
            (Lang::En, HelpPage::Cgroups) => &CGROUPS_EN,
            (Lang::En, HelpPage::Apps) => &APPS_EN,
            (Lang::En, HelpPage::Detail) => &DETAIL_EN,
            (Lang::En, HelpPage::CgroupDrill) => &DRILL_EN,
            (Lang::Zh, HelpPage::Procs) => &PROCS_ZH,
            (Lang::Zh, HelpPage::Cgroups) => &CGROUPS_ZH,
            (Lang::Zh, HelpPage::Apps) => &APPS_ZH,
            (Lang::Zh, HelpPage::Detail) => &DETAIL_ZH,
            (Lang::Zh, HelpPage::CgroupDrill) => &DRILL_ZH,
        }
    }
}

const PROCS_EN: Doc = Doc {
    title: "Processes",
    sections: &[
        Section {
            heading: "Header / status",
            body: &[
                "mem = MemTotal - MemAvailable. It is not the sum of process RSS.",
                "avail = MemAvailable, cached = page cache, swap = used/total.",
                "sweep/pss = sampler time; coverage = fresh PSS PIDs / eligible PIDs and their PSS sum.",
                "n/a (needs root) = EACCES; psi some/full = 10 s memory-stall averages; age = snapshot age.",
            ],
        },
        Section {
            heading: "Columns",
            body: &[
                "RSS (statm) counts shared pages; PSS (smaps_rollup) apportions them; USS is private only.",
                "PSS/RSS >80% is highlighted: little sharing, not an error.",
                "Δ/s is the RSS change per 1 s tick; positive is red, negative green.",
                "SHARE length is relative to the largest visible RSS; colour is relative to total RAM (>15% red, >5% yellow).",
                "PSS MIX is a 16-cell bar: anon / file / shmem / swap-pss, at least one cell per nonzero category. The footer is the legend.",
            ],
        },
        Section {
            heading: "Markers and keys",
            body: &[
                "— not sampled yet; n/a unreadable (EACCES or a kernel thread); ~ PSS older than 6 s.",
                "▸ selected row. t theme, l language, ? help.",
                "Mouse: click a row to select, click it again to open detail; wheel scrolls; footer buttons are clickable.",
            ],
        },
    ],
};

const PROCS_ZH: Doc = Doc {
    title: "进程",
    sections: &[
        Section {
            heading: "顶部状态",
            body: &[
                "mem = MemTotal - MemAvailable，不是进程 RSS 之和。",
                "avail = MemAvailable；cached = 页缓存；swap = 已用/总量。",
                "sweep/pss = 采样耗时；coverage = 新鲜 PSS 进程数 / 可读进程数，以及其 PSS 之和。",
                "n/a (needs root) = EACCES；psi some/full = 10 秒内存停顿均值；age = 快照年龄。",
            ],
        },
        Section {
            heading: "列",
            body: &[
                "RSS（statm）包含共享页；PSS（smaps_rollup）按共享比例分摊；USS 只算私有页。",
                "PSS/RSS >80% 会标黄：说明几乎不共享，不是错误。",
                "Δ/s 是每个 1 秒采样的 RSS 变化；正数红色，负数绿色。",
                "SHARE 长度相对当前可见的最大 RSS；颜色相对总内存（>15% 红，>5% 黄）。",
                "PSS MIX 固定 16 格：anon / file / shmem / swap-pss，非零类别至少 1 格；footer 有图例。",
            ],
        },
        Section {
            heading: "标记与按键",
            body: &[
                "— 尚未采样；n/a 无法读取（EACCES 或内核线程）；~ PSS 超过 6 秒未更新。",
                "▸ 当前选中行。t 主题，l 语言，? 帮助。",
                "鼠标：点击行选中，再点一次打开详情；滚轮滚动；footer 按钮可点击。",
            ],
        },
    ],
};

const CGROUPS_EN: Doc = Doc {
    title: "cgroups v2",
    sections: &[
        Section {
            heading: "Columns",
            body: &[
                "CURRENT = memory.current; ANON/FILE/KERNEL/SLAB come from memory.stat; MAX = memory.max.",
                "Values are hierarchical: a parent already includes its children, so never sum levels.",
                "slab is part of kernel; shmem is part of file.",
                "PSI10 = memory.pressure some avg10; yellow >=5%, red >=20%.",
                "OOM is cumulative; ▲ marks new kills since the previous scan. max means unlimited.",
            ],
        },
        Section {
            heading: "Keys",
            body: &[
                "Enter lists the processes in the selected subtree; Esc returns.",
                "s changes the sort (current or composition), r reverses, f filters paths.",
                "l language, ? help.",
                "Mouse: click a row to select, click it again to open the drill-down; wheel scrolls; footer buttons are clickable.",
            ],
        },
    ],
};

const CGROUPS_ZH: Doc = Doc {
    title: "cgroup v2",
    sections: &[
        Section {
            heading: "列",
            body: &[
                "CURRENT = memory.current；ANON/FILE/KERNEL/SLAB 来自 memory.stat；MAX = memory.max。",
                "数值是层级累计的：父 cgroup 已包含子 cgroup，不能跨层相加。",
                "slab 属于 kernel；shmem 属于 file。",
                "PSI10 = memory.pressure some avg10；5% 黄色，20% 红色。",
                "OOM 是累计值；▲ 表示距上次扫描新增。max 表示不限制。",
            ],
        },
        Section {
            heading: "按键",
            body: &[
                "Enter 列出选中子树内的进程；Esc 返回。",
                "s 切换排序（current 或构成），r 反向，f 过滤路径。",
                "l 语言，? 帮助。",
                "鼠标：点击行选中，再点一次下钻；滚轮滚动；footer 按钮可点击。",
            ],
        },
    ],
};

const APPS_EN: Doc = Doc {
    title: "Applications",
    sections: &[
        Section {
            heading: "Treemap",
            body: &[
                "Area is the selected metric: PSS by default; s cycles PSS -> USS -> RSS.",
                "RSS overcounts shared pages; the title says so.",
                "Groups: app-*.scope -> exe -> comm; the long tail is folded into others (N).",
                "Np is the process count in a group. The right panel is a 120-sample history.",
            ],
        },
        Section {
            heading: "Keys",
            body: &[
                "Enter drills into the selected group's processes; Esc clears that filter.",
                "Narrow or short terminals fall back to a list instead of the treemap.",
                "s metric, f filter, t theme, l language, ? help.",
                "Mouse: click a tile/row to select, click it again to drill; wheel scrolls; footer buttons are clickable.",
            ],
        },
    ],
};

const APPS_ZH: Doc = Doc {
    title: "应用",
    sections: &[
        Section {
            heading: "treemap",
            body: &[
                "面积默认是 PSS；按 s 依次切换 PSS -> USS -> RSS。",
                "RSS 会把共享页重复计算，标题会标注。",
                "分组：app-*.scope -> exe -> comm；长尾折叠为 others (N)。",
                "Np 是该组的进程数；右侧面板是 120 点历史。",
            ],
        },
        Section {
            heading: "按键",
            body: &[
                "Enter 下钻到该应用的进程；Esc 清除过滤。",
                "终端过窄或过矮时回退为列表视图。",
                "s 指标，f 过滤，t 主题，l 语言，? 帮助。",
                "鼠标：点击图块/行选中，再点一次下钻；滚轮滚动；footer 按钮可点击。",
            ],
        },
    ],
};

const DETAIL_EN: Doc = Doc {
    title: "VMA detail",
    sections: &[
        Section {
            heading: "Table",
            body: &[
                "Mappings are aggregated by name; PSS/USS/anon/swap/size/VMAs are sums.",
                "Enter expands a group into individual VMAs with address range and permissions.",
                "z/Z collapse or expand all; f filters mapping names; Esc leaves detail.",
                "PRIV|SHARED bar: USS vs (PSS - USS), scaled to the largest group.",
                "Mouse: click a mapping row to select, click it again to expand its VMAs.",
            ],
        },
        Section {
            heading: "Summary",
            body: &[
                "pss by kind = Pss_Anon + Pss_File + Pss_Shmem; SwapPss is listed separately.",
                "THP and KSM are informational, not part of the four mix segments.",
                "If smaps is unreadable, the reason is shown instead of zeros.",
            ],
        },
    ],
};

const DETAIL_ZH: Doc = Doc {
    title: "VMA 详情",
    sections: &[
        Section {
            heading: "表格",
            body: &[
                "映射按名称聚合；PSS/USS/anon/swap/size/VMAs 都是求和值。",
                "Enter 把一组展开成逐 VMA，显示地址区间和权限。",
                "z/Z 全部收起/展开；f 过滤映射名；Esc 退出详情。",
                "PRIV|SHARED 条：USS 与 (PSS - USS)，按最大组缩放。",
                "鼠标：点击映射行选中，再点一次展开逐 VMA。",
            ],
        },
        Section {
            heading: "摘要",
            body: &[
                "pss by kind = Pss_Anon + Pss_File + Pss_Shmem；SwapPss 单独列出。",
                "THP 和 KSM 仅供参考，不属于构成条的四段。",
                "smaps 读不到时显示原因，而不是用 0 代替。",
            ],
        },
    ],
};

const DRILL_EN: Doc = Doc {
    title: "cgroup drill-down",
    sections: &[
        Section {
            heading: "What is listed",
            body: &[
                "Only processes in the selected cgroup subtree are shown.",
                "The summary reports current memory, limit and PSI for that cgroup.",
                "PID/COMM/RSS/PSS/PSS-RSS/SHARE have the same meaning as the process view.",
            ],
        },
        Section {
            heading: "Keys",
            body: &[
                "Esc returns to the cgroup list; Tab switches to processes.",
                "j/k scrolls the member list; l language, ? help.",
                "Mouse: click a member row to select; wheel scrolls; footer buttons are clickable.",
            ],
        },
    ],
};

const DRILL_ZH: Doc = Doc {
    title: "cgroup 下钻",
    sections: &[
        Section {
            heading: "列出范围",
            body: &[
                "只显示选中 cgroup 子树内的进程。",
                "摘要显示该 cgroup 的当前内存、上限和 PSI。",
                "PID/COMM/RSS/PSS/PSS-RSS/SHARE 的含义与进程视图一致。",
            ],
        },
        Section {
            heading: "按键",
            body: &[
                "Esc 返回 cgroup 列表；Tab 切换到进程视图。",
                "j/k 滚动成员列表；l 语言，? 帮助。",
                "鼠标：点击成员行选中；滚轮滚动；footer 按钮可点击。",
            ],
        },
    ],
};

#[cfg(test)]
mod tests {
    use super::{HelpPage, Lang, T};

    #[test]
    fn parses_languages_and_locales() {
        assert_eq!(Lang::parse("en"), Some(Lang::En));
        assert_eq!(Lang::parse("ZH"), Some(Lang::Zh));
        assert_eq!(Lang::parse("zh_CN.UTF-8"), Some(Lang::Zh));
        assert_eq!(Lang::parse("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(Lang::parse("fr"), None);
    }

    #[test]
    fn toggle_round_trips() {
        assert_eq!(Lang::En.toggle(), Lang::Zh);
        assert_eq!(Lang::Zh.toggle(), Lang::En);
    }

    #[test]
    fn every_page_has_bilingual_help() {
        for page in [
            HelpPage::Procs,
            HelpPage::Cgroups,
            HelpPage::Apps,
            HelpPage::Detail,
            HelpPage::CgroupDrill,
        ] {
            let en = T(Lang::En).doc(page);
            let zh = T(Lang::Zh).doc(page);
            assert!(!en.sections.is_empty(), "{page:?}");
            assert!(!zh.sections.is_empty(), "{page:?}");
            assert_ne!(en.title, zh.title, "{page:?}");
            assert!(en.sections.iter().all(|section| !section.body.is_empty()));
            assert!(zh.sections.iter().all(|section| !section.body.is_empty()));
        }
    }
}
