# mtop 需求与设计约束

Linux 上"内存"这件事，`htop` 只给 RSS，任务管理器把共享页重复计算。mtop 的目标只有一个：**让"到底是谁占了内存"这个问题在终端里一次看清**，并且用 PSS/USS 给出不重复计算的答案，再下钻到具体的 VMA。

## 1. 用户与场景

| 场景 | 需要什么 | mtop 的做法 |
|---|---|---|
| 服务器内存吃紧，找元凶 | 按真实占用排序，而不是把共享库算 N 遍 | PSS 列 + RSS/PSS 比值 |
| 容器/桌面里浏览器一堆进程 | 看清同一进程组内的分布 | 进程表 + VMA 映射聚合视图 |
| 怀疑某个进程泄漏 | 看增长而不是快照 | Δ/s 列 + 焦点进程 RSS 趋势图 |
| 想知道一个进程的 500 MB 到底在哪 | 按映射聚合，而不是 2000 行 VMA | 按映射名聚合的 PSS/USS 表 |

## 2. 非目标（M1 明确不做）

- 跨平台。只用 `/proc`，Linux only。
- CPU / 网络 / 磁盘监控。那是 `btm`/`htop` 的活。
- eBPF、tracepoint、堆分配器插桩。只看内核已有的计数器。
- 写操作：kill / renice / drop_caches。
- 常驻 daemon、远程采集、历史持久化。
- 配置文件。命令行参数 + 按键即可。

## 3. 复用的社区资产（不重复造轮子）

| 项目 | 许可证 | 关系 | 结论 |
|---|---|---|---|
| [ratatui](https://github.com/ratatui/ratatui) 0.30.2 | MIT | 依赖 | TUI 框架，pin 死小版本 |
| [procfs](https://github.com/eminence/procfs) 0.18 | MIT/Apache-2.0 | 依赖 | **全部** `/proc` 解析：`smaps_rollup`、`smaps`（含 `MMapPath` 类型化分类）、`meminfo`、`statm`、`cmdline` |
| [bottom](https://github.com/ClementTsang/bottom) 14.1k★ | MIT | 架构/交互参考 | 不 fork：它的 `ProcessHarvest` 只有单个 `mem_usage` 字段，加 PSS 要动采集层+列+排序+配置；且带 5 平台与 GPU 配置面 |
| [jtop-pss](https://crates.io/crates/jtop-pss) 0.5.0-beta | MIT | 同类前作 | 2057 行单文件，已证明 PSS/USS 监控可行；无库 API，只作参考 |
| [wora-observability-linux-mm](https://crates.io/crates/wora-observability-linux-mm) 0.2.0 | MIT/Apache-2.0 | M2 升级路径 | 类型化的 VMA 元数据、KSM、hugepage、pagemap；M1 不需要 |
| smem (Python) | GPL-2 | 语义参考 | PSS/USS 的定义与"别把共享页算 N 遍"的教训 |

**复用边界（重要）**：复杂格式（smaps 有 20+ 字段、VMA 头部有 6 段）交给 `procfs`；单行单值的 `statm` 直接读——`procfs::Process::new` 每个 pid 多一次 `openat(O_PATH)`，实测把全系统 sweep 从 15 ms 拉到 43 ms，不值得。

## 4. 功能需求与验收标准

| ID | 需求 | 验收标准 |
|---|---|---|
| F1 | 进程表：PID / COMM / RSS / PSS / USS / PSS÷RSS / Δ·s⁻¹ / 占比条 | `tests/render.rs::process_table_renders_sorted_with_pss_gaps_marked` |
| F2 | 缺失的 PSS 显示 `—`，不显示 0 | 同上（断言存在 `—`） |
| F3 | 排序：RSS/PSS/USS/Δ/PSS÷RSS/PID/COMM，可反向 | `sort_by_pss_reorders...`、`keyboard_navigation_is_clamped` |
| F4 | 过滤：按进程名子串或 PID 前缀 | `filter_matches_comm_and_pid` |
| F5 | 详情：按映射名聚合 PSS/USS/虚拟大小/VMA 数 | `detail_view_shows_grouped_vmas` + 实机端到端（2292 VMAs → 286 组） |
| F6 | 焦点进程 RSS 趋势 sparkline（120 点） | 实机截图；数据源为 UI 侧环形缓冲 |
| F7 | 诚实表头：内存用量取自 `/proc/meminfo`；另显示 sweep/pss 耗时、PSS 覆盖率、样本年龄 | 实机帧：`coverage 205/205 = 5.85G 237 n/a` |
| F8 | 权限失败时显示原因，不显示空面板 | `detail_error` → "vm mappings unavailable / /proc/N/smaps: …" |
| F9 | `--pid PID` 启动即选中 | 实机：`--pid 579585` → 首行 ▸ chrome |
| F10 | 不可读进程区分原因：`EACCES`（需要 root，提示 `n/a (needs root)`）与 `ESRCH/无 mm`（内核线程，root 也无解，仅标 `n/a`）；两者都负缓存 30 s、不占预算 | `denied_pids_do_not_consume_the_budget_until_the_denial_expires`、`unreadable_processes_are_classified_by_reason` |
| F11 | 按 PSS/USS/比值排序时，覆盖率未满则标题标注 `ranking incomplete` | `render` 断言 + 实机帧 |
| F12 | 进程内存构成：anon / file / shmem / swap-pss / THP / KSM，表格里用堆叠条、详情页给出数值 | 实机：chrome `anon 243M file 57M shmem 4.3M`，与 `/proc/PID/smaps_rollup` 一致 |
| F13 | 详情页可过滤映射名（`f`），与进程过滤互不干扰 | `detail_filter_narrows_mappings`、`editing_targets_the_detail_filter...` |
| F14 | `--json` 输出一次性快照：稳定按 PID 排序、显式 `_kb` 单位、带 schema 版本与 `self_pid` | `tests/json.rs` 通过真实二进制校验 |
| F15 | 详情页把映射展开为逐 VMA（地址区间、权限、PSS/USS/anon/swap/size） | `enter_expands_a_group_into_its_individual_vmas`、实机 1266 个 VMA |
| F16 | `--diff A.json B.json` 按 \|ΔRSS\| 排序输出，标注 new/gone，排除快照写者自身 | `tests/diff.rs`（4 个场景） |
| F17 | `Tab` 切换 cgroup v2 视图：path/current/anon/file/kernel/slab/max/oom，按 current 或构成排序、可过滤 | `tab_switches_to_the_cgroup_view`、`cgroup_view_sorts_and_filters`、实机 174 个 cgroup |
| F18 | `--json` 附带 `cgroups` 数组（path/current/anon/file/kernel/slab/shmem/limit/oom_kills） | `tests/json.rs` |
| F19 | cgroup 视图 `Enter` 下钻出子树内进程（PID/COMM/RSS/PSS/占比），`Esc` 返回 | `cgroup_drill_lists_members_of_the_subtree`、`drill_down_uses_subtree_membership` |
| F20 | OOM 事件增量：列显示累计值，采样间隔内新增时标 `▲` 且标题汇总；首帧不算增量 | `tab_switches_to_the_cgroup_view`（`2▲` + 标题断言） |
| F21 | `--diff --min-delta SIZE --top N`：过滤小变化、限制行数；非法值退出码 2 | `tests/diff.rs`（阈值/条数/坏参数） |
| F22 | PSI：表头显示系统 `some/full avg10`，cgroup 视图有 `PSI10` 列并按 5%/20% 上色，右上角画选中项走势（固定 0–100% 刻度） | `pressure_parses_some_and_full`、`cgroup_psi_history_is_recorded_and_plotted`、渲染断言 `psi 1.5/0.2%` 与 `12.5%` |
| F23 | `--json` 输出 `pressure{some10,full10}` 与每个 cgroup 的 `psi_some10/psi_full10` | `tests/json.rs` |
| F24 | `--json --fields a.b,c` 点号路径投影（数组按记录投影、null 段保留、`schema`/`at_unix_ms` 恒在、未知路径退出 2） | `tests/json.rs` + 4 个投影单测 |
| F25 | OOM 时间序列：每个扫描点采样 `oom_recent`，120 点窗口，下钻页用文本 sparkline 呈现 | `text_spark_renders_a_normalised_series`、`cgroup_psi_history_is_recorded_and_plotted`、实机 `oom ▁▁▁▁ 0 total` |
| F26 | apps 视图：面积 ∝ 应用内存，分组按 cgroup app scope → exe → comm，长尾折叠为 `others` | `apps.rs` 单测（scope 解析/合并/折叠）、`apps_view_groups_processes_and_drills_into_the_process_table` |
| F27 | apps 视图选中并用 `Enter` 下钻到该应用的进程（`Esc` 清除过滤） | 同上 + `selected_row_is_tinted...` 同源的渲染测试 |
| F28 | treemap 布局不变量：在界内、互不重叠、面积成比例 | `treemap_tiles_stay_inside_and_do_not_overlap`、`treemap_area_is_proportional_within_a_few_percent` |
| F29 | apps 视图右上角显示选中应用的历史（120 点），按指标分别存储；RSS 模式在标题标注"overcounts shared" | `apps_history_tracks_each_metric_separately`、实机 `▅▇▇█…` |
| F30 | 七套内置配色：`default`、`btop`、`dracula`、`gruvbox-dark`、`kanagawa-wave`、`light`、`ansi16`；后四套的颜色取自 btop 自带主题文件；`--theme`、`MTOP_THEME`、运行时 `t` 循环；不引入配置文件；默认配色与旧版逐色一致；按终端能力自动以真彩/256/16 色渲染（btop 的 6×6×6 + 灰阶量化规则），`--color auto\|truecolor\|256\|16` 可覆盖 | `theme.rs` 单测（含量化）、`theme_key_cycles_...`、`ansi256_rendering_uses_indexed_colours_only`、`unknown_colour_mode_is_a_usage_error` |

## 5. 数据来源与精度

| 指标 | 来源 | 公式 | 陷阱 |
|---|---|---|---|
| RSS | `/proc/PID/statm` 字段 2 | `pages × page_size` | 计数器口径，与 `smaps_rollup` 的 `Rss:` 可能有几个百分点差异 |
| PSS | `/proc/PID/smaps_rollup` | 内核按共享页比例分摊 | 需要走页表，代价高 |
| USS | `smaps_rollup` | `Private_Clean + Private_Dirty` | 不含 `Private_Hugetlb` |
| anon | `smaps_rollup` | `Anonymous:` | — |
| 系统内存 | `/proc/meminfo` | `MemTotal - MemAvailable` | **绝不用进程行求和**：PSS 只覆盖一部分进程 |
| VMA 分组 | `/proc/PID/smaps` | 按 `MMapPath` 聚合 | `Size` 是虚拟大小，含未提交的稀疏映射（见过 100 G 的 `[anon]`） |
| cgroup 内存 | `/sys/fs/cgroup/**/memory.current` + `memory.stat` | `current ≈ anon + file + kernel` | **层级累计**，父子不可相加；`slab ⊂ kernel`、`shmem ⊂ file`；仅 cgroup v2 |
| 应用分组 | `/proc/PID/cgroup` 的 `app-*.scope`；回退 `/proc/PID/exe`；再回退 `comm` | 去掉尾部 `-<pid>`、还原 `\x2d`、同一应用多 scope 合并 | `others (N)` 不可下钻；exe 回退会把两个 Chromium 合并（cgroup 口径不会） |
| 进程归属 | `/proc/PID/cgroup` | `0::<path>` 单行解析 | 与 cgroup 列表同频（5 s）刷新；下钻按**子树**匹配（前缀 + 边界） |
| 内存压力 | `/proc/pressure/memory`、`/sys/fs/cgroup/**/memory.pressure` | `some avg10`（10 s 内有任务因内存停顿的时间占比） | 系统级每 tick 读；5% / 20% 是经验阈值；`full` 只在全部非空闲任务都停顿时增长 |
| PSS 构成 | `/proc/PID/smaps_rollup` | `Pss_Anon` / `Pss_File` / `Pss_Shmem`（三者之和 = `Pss`） | **逐 VMA 的 `smaps` 不输出这三个字段**，早期实现按 VMA 求和恒为 0——构成必须取 rollup |
| swap-pss | `smaps_rollup` | `SwapPss`（按共享度加权） | 与 `Swap`（实际换出量）不是同一个量，UI 分开显示 |
| THP / KSM | `smaps_rollup` | `AnonHugePages` / `KSM` | 大页会被算进 anon，KSM 合并的页仍计入 PSS |

**PSS 覆盖率的语义**：`smaps_rollup` 需要 ptrace 读权限。实测 439 个 PID 中：**176 个内核线程（无 mm，root 也读不到）**、**59 个其他用户进程（root 可读）**、204 个本用户进程。因此：

- 分母 = `procs − no_mm`（不含内核线程），否则永远到不了 100%。
- `n/a (needs root)` 只统计 `EACCES` 的那一类，root 下应归零；内核线程的 `n/a` 不带 sudo 提示。
- 用 root 验证的完整步骤与预期数值见 [VERIFY-ROOT.md](VERIFY-ROOT.md)。

`procfs::Meminfo` 的单位是**字节**，UI 用 kB——这条转换有专门的回归测试（`meminfo_is_normalized_to_kb`），因为漏掉 `/1024` 的后果是表头显示 15.7 TB。

## 6. 性能预算（本机实测：8 核 / 15 GiB / 465 个 PID）

| 路径 | 单次 | 频率 | 每次 tick 预算 |
|---|---|---|---|
| `statm` + `comm` 全量 sweep | 15–25 ms（负载高时实测 41 ms） | 1 Hz | 25 ms |
| `smaps_rollup` 单进程 | 1.5 ms 均值，chrome 16–19 ms | 受限轮转 | 150 ms |
| `smaps` 全量（2292 VMA） | 31–42 ms 裸解析，65 ms 经 `procfs` | 焦点进程，2 s 一次 | 33 ms 摊销 |
| `/proc/meminfo` | <1 ms | 5 s 一次 | ~0 |

稳态开销约 **200–250 ms/s ≈ 一个核的 20–25%**（最坏情况）。实测：修掉"每 tick 重试不可读进程"之前，覆盖率卡在 `213/447`；修复后 12 s 内收敛到 `213/218`（可读进程全覆盖），且实测排序结果与独立 Python 实现逐行一致。这不是"性能优化没做"，而是"读页表本身就是被测系统的负载"——所以 `smaps_rollup` 必须受预算约束，`smaps` 必须按需。预算常量在 `src/sampler.rs`：`PSS_BUDGET`、`PSS_MAX_AGE`、`DETAIL_MAX_AGE`。

## 7. 交互契约

| 按键 | 行为 |
|---|---|
| `j`/`k`、`↑`/`↓` | 移动选择（详情视图内滚动映射） |
| `PgUp`/`PgDn` | 翻 10 行 |
| `g`/`G`、`Home`/`End` | 顶部 / 底部 |
| `s` | 循环排序键（重置选择） |
| `r` | 反向排序 |
| `t` | 循环配色主题（`default` → `btop` → `dracula` → `gruvbox-dark` → `kanagawa-wave` → `light` → `ansi16`；立即重绘） |
| `Tab` | 进程视图 → cgroup 视图 → apps 视图（循环；切换时退出详情与下钻） |
| `Enter`（cgroup） | 列出该 cgroup 子树内的进程；`Esc` 返回列表 |
| `Enter`（apps） | 下钻到该应用的进程（进程表加 `app: <name>` 过滤），`Esc` 清除 |
| `s`（apps） | 面积指标 PSS → USS → RSS |
| `f` | 过滤输入；`Enter` 应用，`Esc` 取消（进程视图过滤名/PID，cgroup 视图过滤路径，详情页过滤映射名） |
| `Esc`（详情） | 有映射过滤时先清除过滤，再按一次退出详情 |
| `Enter` | 进入详情；在详情内展开/收起映射（`z`/`Z` 全部收起/展开） |
| `Esc` | 清除过滤 / 返回 |
| `q`、`Ctrl-C` | 退出 |

## 8. 已知风险

1. **PSS 覆盖需要时间填满**：预算按"未知优先、其次最旧"分配，全量可读进程约 5 s 填满（长期稳态覆盖满）。填写期间按 PSS 排序会有误差，标题会显式标注 `ranking incomplete`；`n/a` 行按 0 参与排序。
2. **测量即扰动**：读 `smaps` 会拿 `mmap_lock` 走页表，对大进程有明显内核开销；因此详情页不随 tick 刷新。
3. **权限**：非本用户进程的 `smaps` 可能 `EACCES`，详情页会显示原因。
4. **PID 复用**：扫描期间进程退出/复用 PID，某一行可能短暂混合两个进程的数据。
4b. **读不到 ≠ 不占内存**：`n/a` 的进程（多为 root 进程）仍有 RSS，只是拿不到 PSS。要全量 PSS 就用 `sudo` 运行；这条限制来自内核的 ptrace 权限模型，不是实现缺陷。
5. **内核线程**：`smaps_rollup` 为空，PSS 显示 `—`，不是 0。
6. **快照的 new/gone 语义**：`--diff` 只比较两份快照，进程在两次采样之间重启会同时表现为 gone + new（PID 变化）；写快照的 `mtop` 自身按 `self_pid` 排除。
7. **root 路径已实测**（原为风险，现为已验证）：`sudo scripts/verify-root.sh` 输出 `procs=447 covered=264 denied=0 no_mm=183`，`264 + 183 = 447`，`denied` 归零、无 `PSS > RSS`；TUI 表头从 `203/262 + 59 n/a (needs root)` 变为 `265/265` 且提示消失。记录见 [VERIFY-ROOT.md](VERIFY-ROOT.md#6-实测记录2026-10-31)。
8. **ratatui 0.30 拆包后的 API 漂移**：`Cargo.toml` 用 `=0.30.2` 锁定，升级需单独评估。

## 9. 交付状态

| 项 | 状态 |
|---|---|
| 功能需求 F1–F23 | 全部实现，逐项有测试或实机抓帧 |
| 自动化测试 / clippy | 88 tests，0 warning |
| 单文件分发 | `cargo build --release` → 1.4 MB，无运行时依赖 |
| LICENSE | MIT（与 `Cargo.toml` 一致） |
| `--version` / `--help` | 已提供，`tests/cli.rs` 覆盖 |
| root 路径 | 已实测：`procs=447 covered=264 denied=0 no_mm=183`（见 VERIFY-ROOT.md §6） |
| CI | GitHub Actions：`fmt` / `clippy -D warnings` / 全量测试；打包任务构建 musl 二进制并校验 deb/rpm 的载荷与架构 |
| 发布产物 | `v0.2.0` 已发布：amd64/arm64 的 `.deb` 与 `.rpm`、静态 musl `.tar.gz`、按目标生成的 `SHA256SUMS` |
| 仓库 | https://github.com/baka-world/mtop（public，MIT） |

## 10. 里程碑

| 阶段 | 状态 | 内容 |
|---|---|---|
| M1 | 完成 | 两层采样；进程表（RSS/PSS/USS/Δ）；过滤/排序；趋势 |
| M1.1 | 完成 | PSS 覆盖率策略：未知优先 → 最旧优先；EACCES 负缓存 30 s；分母排除无 mm 进程；陈旧值 `~` |
| M2 | 完成 | PSS 构成（anon/file/shmem/swap-pss/THP/KSM）；VMA 分组展开；`--json` + `--fields`；`--diff`（阈值/条数/self_pid 排除） |
| M3 | 完成 | cgroup v2 视图与下钻；OOM 增量与时间序列；PSI（系统级 + 每 cgroup + 走势） |
| M4 | 完成 | apps treemap：三级分组、others 折叠、PSS/USS/RSS 面积、下钻到进程、窄终端回退、每 app 趋势 |

### 未做（都是可选增强，不是缺口）

- 历史窗口可配置（现在进程/apps 是 1 Hz × 120 点 = 2 分钟，cgroup/PSI 是 5 s × 120 点 = 10 分钟，两条时间轴不一致）。
- 历史窗口可配置（见上）。
- `--json` 输出 apps 分组结果。
- `--json` 输出 apps 分组结果（当前只有 procs/cgroups）。

