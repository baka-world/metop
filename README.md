# mtop

Linux 内存分析 TUI：按 RSS / **PSS** / **USS** 看进程，按映射聚合下钻到 VMA。

`htop` 只给 RSS；把共享库算 N 遍的"内存排行"会骗人。mtop 用内核的 PSS 口径回答"谁真的占了内存"，并给出每个进程的内存花在了哪些映射上。

```
 mtop  442 procs  mem 8.13G/15.36G  avail 7.23G  cached 6.15G  swap 3.28G/16.00G
 sweep 12ms pss 157ms coverage 205/205 = 5.92G  237 n/a (sudo) psi 0.0/0.0% age 867ms
┌ procs  sort RSS ↓  filter - ─────────────────────────────────────────────────────┐
│     PID COMM                   RSS     PSS     USS PSS/RSS     Δ/s  SHARE  PSS MIX│
│▸ 579585 chrome                524M    346M    320M     66%      0  ██████ ██████████
│  623313 omp                   397M    349M    328M     88% +  0.1M █████  ████████████
│    4551 dnf5daemon-serv       354M     n/a     n/a      —       0  ████
│  512136 jetbrains-toolb       308M    281M    279M     91% -  0.0M ████   ████████████
└──────────────────────────────────────────────────────────────────────────────────┘
 q quit │ j/k move │ g/G ends │ s sort (RSS) │ r reverse │ f filter │ Enter vma detail
```

## 构建与运行

```sh
cargo build --release
./target/release/mtop            # 全系统
./target/release/mtop --pid 1234 # 启动即选中某个进程
./target/release/mtop --json     # 一次性快照（可 diff、可 jq）
./target/release/mtop --json --pid 1234   # 附带该进程的 VMA 明细
mtop --json --fields procs.pid,procs.rss_kb,procs.pss.pss_kb   # 只导出需要的字段
mtop --diff a.json b.json        # 比较两份快照，按 |ΔRSS| 排序
mtop --diff a.json b.json --min-delta 50M --top 10   # 只看显著变化
mtop --version
```

```sh
mtop --json > a.json; sleep 30; mtop --json > b.json
mtop --diff a.json b.json
```

```
mtop diff  over 4s  (437 -> 438 procs)   15 grew, 3 shrank, 1 new, 0 gone
     PID COMM                          ΔRSS      ΔPSS   RSS now   PSS now  note
  613254 chrome                       -1.2M     -1.2M      348M      171M
    2796 gnome-shell                  +0.2M     +0.2M      288M      235M
```

`--fields` 接受点号路径（`procs.pss.pss_kb` 这种嵌套也行），`schema` 与 `at_unix_ms` 始终保留；未知路径直接退出码 2 并列出可用顶层键。本机实测：全量 194 KB → 三个字段 37 KB。

写快照的那个 `mtop` 进程会被记录为 `self_pid` 并在 diff 中自动排除——否则每次运行都是"new 3 MB / gone 3 MB"的假噪声。`--min-delta` 接受 `50M` / `512k` / `2G` / 纯数字（kB）。

## 按键

| 键 | 作用 |
|---|---|
| `j`/`k`、`↑`/`↓`、`PgUp`/`PgDn` | 移动 |
| `g`/`G` | 顶 / 底 |
| `s` / `r` | 切换排序键 / 反向（cgroup 视图按 CURRENT/ANON/FILE/SLAB 排序） |
| `Tab` | 进程视图 → cgroup 视图 → apps 视图（循环） |
| `f` | 过滤（进程名 / PID / cgroup 路径；详情页中过滤映射名） |
| `Enter` | 进程视图：进入详情；详情内展开/收起映射；cgroup 视图：列出子树内进程 |
| `z` / `Z`（详情） | 全部收起 / 全部展开 |
| `Esc` | 清除过滤 / 返回 |
| `q` | 退出 |

## 设计要点

两层采样，按实测成本分配预算：

| 路径 | 成本 | 频率 |
|---|---|---|
| `/proc/*/statm` 全量 | 15–25 ms（负载高时 ~40 ms） | 1 Hz |
| `/proc/PID/smaps_rollup` | 1.5 ms 均值，大进程 16–19 ms | 受 150 ms/tick 预算约束 |
| `/proc/PID/smaps` | 31–65 ms（2000+ VMA） | 仅焦点进程，2 s 一次 |

`/proc` 解析全部交给 [`procfs`](https://crates.io/crates/procfs) 0.18，UI 用 [`ratatui`](https://crates.io/crates/ratatui) 0.30（版本锁定）。复用了什么、为什么没有 fork `bottom`、以及精度与风险，见 [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md)。

## apps 视图（`Tab` 两次）：按应用的内存地图

进程表回答"哪个进程大"，cgroup 视图回答"哪个边界大"，apps 视图回答**"哪个应用占了多少"**——把同一应用的几十个进程合并，用**面积 ∝ PSS** 的 treemap 画出来。

```
┌ apps  metric PSS  filter -  (20 groups · 5.40G) ──────────────────────────────────┐
│ com.google.Chrome                     gnome-chatgpt        ghostty-…             │
│ 1.57G · 30p                           611M · 17p           529M · 12p             │
│                                                                                    │
│                                       gnome-trilium        node-22                │
│                                       277M · 6p            268M · 1p              │
└────────────────────────────────────────────────────────────────────────────────────┘
```

- **分组口径**（三级回退）：systemd 的 `app-*.scope`（去掉末尾 PID、还原 `\x2d`，同一应用的多个 scope 合并）→ 可执行文件名（去掉 ` (deleted)`）→ `comm`。Chrome 就是典型：两个 scope、30 个进程，合并成一个 1.57G 的块。
- **为什么用 PSS**：同一时刻 chrome 的 RSS 相加 4.3G、PSS 1.57G；全机 RSS 之和是 PSS 的 2.7 倍。用 RSS 画图会把多进程应用放大成假地图。`s` 在 PSS / USS / RSS 间切换（USS ≈ "关掉它能省多少"）。
- **长尾**：占比 < 0.5% 的应用合并成 `others (N)`，否则 327 个应用会碎成一堆 1 像素的条。
- **下钻**：`Enter` 跳到进程视图并只显示该应用的进程，标题显示 `app: <name>`，`Esc` 清除。
- **趋势**：右上角是选中应用 120 点的历史，PSS/USS/RSS 三条序列各自独立——切指标不会把不同口径的数据混在一条线里。
- **窄终端**：宽度 < 100 或高度 < 14 时自动退化成按应用排序的列表（`s`/`j/k` 同样可用）。

## cgroup 视图（`Tab`）

进程表回答"哪个进程大"，但容器/服务的边界是 cgroup。`Tab` 切到 cgroup v2 视图，直接读 `/sys/fs/cgroup` 的 `memory.current` / `memory.stat` / `memory.max` / `memory.events`：

```
 cgroups  sort CUR ↓  filter -  (174 of 174)
 CGROUP                           CURRENT      ANON      FILE    KERNEL     SLAB      MAX  OOM
 /user.slice                        9.50G     4.36G     4.72G      417M     255M      max    0
 /user.slice/user-1000.slice        9.33G     4.36G     4.56G      404M     242M      max    0
 /system.slice                       999M      446M      514M       37M      22M      max    0
```

- 数值是**层级累计**的：父 cgroup 已包含子 cgroup，所以列表按 `current` 排序而不是跨层相加。
- 校验恒等式：`current ≈ anon + file + kernel`（`slab` 属于 `kernel`，`shmem` 属于 `file`）。
- `MAX` 显示 `max` 表示无限制；条按 `current/limit` 画，无限制时按最大 cgroup 归一。
- `OOM` 列是累计 `oom_kill`；采样间隔内新增时显示 `2▲` 并在标题给出 `oom +N since last sample`。第一次扫描记为 0 增量，不会误报。
- `PSI10` 是 `memory.pressure` 的 `some avg10`：过去 10 秒里有任务因内存卡住的时长占比。右上角 sparkline 画选中 cgroup 的 PSI 走势（固定 0–100% 刻度，避免 2% 就看成像满格）。**它比 OOM 更早预警**——OOM 是已经出事，PSI 是正在变慢。表头那行给的是系统级 `/proc/pressure/memory`（some/full）。阈值 5% / 20% 只是经验值，不是内核定义。
- 仅支持 cgroup v2（需要 `memory.current`）；v1 或未启用 memory controller 时视图会说明原因。
- `Enter` 下钻：列出该 cgroup **子树**内的进程（PID/COMM/RSS/PSS/PSS÷RSS），按 RSS 排序；`Esc` 返回。下钻页第二行是 OOM 时间序列（每个 5 s 扫描一个采样点，120 点 = 10 分钟窗口）。pid→cgroup 映射来自 `/proc/PID/cgroup`，与 cgroup 列表一起每 5 s 刷新。

```
 /user.slice  current 9.92G · 210 procs in subtree · limit max
┌ procs in cgroup subtree  (Esc back) ──────────────────────────────────────────────┐
│     PID COMM                             RSS       PSS  PSS/RSS  SHARE             │
│▸ 579585 chrome                          520M      343M      66% ██████             │
│  623313 omp                             395M      346M      88% █████              │
└───────────────────────────────────────────────────────────────────────────────────┘
```

## 详情页看什么

表格里的 `PSS MIX` 是按 PSS 加权的构成条：紫=anon、青=file、黄=shmem、蓝=swap（`NO_COLOR` 生效时只剩单色块，这是 crossterm 的标准行为）。

`Enter` 进入某个进程后，按映射聚合（2292 个 VMA → 286 组），并给出**PSS 构成**数值：anon / file / shmem / swap-pss / THP / KSM。

注意口径：`Pss_Anon`/`Pss_File`/`Pss_Shmem` **只在 `smaps_rollup` 中存在**，逐 VMA 的 `smaps` 不输出它们——所以构成取进程级 rollup，映射级用 `Anonymous`/`Swap`/`Pss`。用逐 VMA 求和会得到全 0（踩过）。

## 已知限制

- **PSS 需要权限**：`/proc/PID/smaps_rollup` 走 ptrace 读权限检查。普通用户只能读到自己（及同凭据 / dumpable）的进程；表头把两类"读不到"分开：`n/a (needs root)` 是其他用户的进程（`sudo` 可解），内核线程没有地址空间（root 也无解，只标 `n/a`）。分母 = 进程数 − 内核线程数。`sudo ./target/release/mtop` 后 `n/a (needs root)` 应归零，验证步骤见 [docs/VERIFY-ROOT.md](docs/VERIFY-ROOT.md)。
- **读不到不代表不占内存**：`n/a` 的行仍有 RSS，只是没有 PSS。
- 内核线程的 `smaps_rollup` 为空，恒为 `n/a`。

## 测试

```sh
cargo test --release
```

- `tests/procfs.rs`：对真实 `/proc` 的不变量（PSS ≤ RSS、分组求和、meminfo 单位、statm 与 rollup 一致性）
- `tests/render.rs`：`TestBackend` 把 TUI 渲染成纯文本后断言列、排序、截断、`—`、覆盖率、详情聚合、展开、按键边界
- `tests/json.rs` / `tests/diff.rs`：通过真实二进制校验快照与 diff 的对外契约
- `src/cgroup.rs` 内联单测：`memory.stat`/`memory.max`/`memory.events` 解析（含 `max` 字面量与垃圾行）
