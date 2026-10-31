//! Grouping processes into "apps" and squarified treemap layout.
//!
//! The grouping order matters and is documented in REQUIREMENTS: a systemd
//! `app-*.scope` wins because it encodes the app's own intent (helpers
//! included), then the executable, then comm. Chrome is the motivating case:
//! one desktop app, two scopes and 29 processes.

use std::collections::HashMap;

use ratatui::layout::Rect;

use crate::model::{ProcRow, PssInfo};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppGroup {
    pub name: String,
    pub pss_kb: u64,
    pub uss_kb: u64,
    pub rss_kb: u64,
    pub procs: u32,
    pub pids: Vec<u32>,
    /// The "others" tile collects the long tail and cannot be drilled into.
    pub synthetic: bool,
}

/// systemd escapes '-' as `\x2d` inside unit names.
pub fn unescape_scope(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'x') {
            chars.next();
            let mut hex = String::with_capacity(2);
            for _ in 0..2 {
                if let Some(h) = chars.next() {
                    hex.push(h);
                }
            }
            match u8::from_str_radix(&hex, 16) {
                Ok(v) => out.push(v as char),
                Err(_) => {
                    out.push('\\');
                    out.push('x');
                    out.push_str(&hex);
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `ghostty-surface-transient-600564` -> `ghostty-surface-transient`.
fn strip_trailing_pid(s: &str) -> &str {
    match s.rsplit_once('-') {
        Some((head, tail))
            if !head.is_empty() && !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) =>
        {
            head
        }
        _ => s,
    }
}

fn scope_app_name(cgroup_path: &str) -> Option<String> {
    let last = cgroup_path.rsplit('/').next().unwrap_or("");
    let body = last.strip_prefix("app-")?.strip_suffix(".scope")?;
    let name = strip_trailing_pid(&unescape_scope(body)).to_string();
    (!name.is_empty()).then_some(name)
}

pub fn app_name(comm: &str, cgroup: Option<&str>, exe: Option<&str>) -> String {
    if let Some(name) = cgroup.and_then(scope_app_name) {
        return name;
    }
    if let Some(path) = exe {
        let base = path.rsplit('/').next().unwrap_or(path);
        let base = base.trim_end_matches(" (deleted)");
        if !base.is_empty() {
            return base.to_string();
        }
    }
    comm.to_string()
}

/// One group per app, biggest PSS first.
pub fn group(
    rows: &[ProcRow],
    pss: &HashMap<u32, PssInfo>,
    pid_cgroup: &HashMap<u32, String>,
    pid_exe: &HashMap<u32, String>,
) -> Vec<AppGroup> {
    let mut groups: HashMap<String, AppGroup> = HashMap::new();
    for row in rows {
        let name = app_name(
            &row.comm,
            pid_cgroup.get(&row.pid).map(String::as_str),
            pid_exe.get(&row.pid).map(String::as_str),
        );
        let g = groups.entry(name.clone()).or_insert_with(|| AppGroup {
            name,
            ..Default::default()
        });
        g.procs += 1;
        g.rss_kb += row.rss_kb;
        g.pids.push(row.pid);
        if let Some(info) = pss.get(&row.pid) {
            g.pss_kb += info.rollup.pss_kb;
            g.uss_kb += info.rollup.uss_kb;
        }
    }
    let mut out: Vec<AppGroup> = groups.into_values().collect();
    for g in &mut out {
        g.pids.sort_unstable();
    }
    sort_groups(&mut out);
    out
}

fn sort_groups(groups: &mut [AppGroup]) {
    groups.sort_by(|a, b| b.pss_kb.cmp(&a.pss_kb).then_with(|| a.name.cmp(&b.name)));
}

/// Fold everything below `min_share` of the total into one "others" tile, so
/// the map does not dissolve into slivers (327 apps, top 12 = 89%).
pub fn collapse(groups: Vec<AppGroup>, min_share: f64) -> Vec<AppGroup> {
    let total: u64 = groups.iter().map(|g| g.pss_kb).sum();
    if total == 0 || !(0.0..1.0).contains(&min_share) {
        return groups;
    }
    let threshold = (total as f64 * min_share) as u64;
    let mut out = Vec::with_capacity(groups.len());
    let mut others = AppGroup {
        name: String::new(),
        synthetic: true,
        ..Default::default()
    };
    let mut hidden = 0usize;
    for g in groups {
        if g.pss_kb >= threshold {
            out.push(g);
        } else {
            hidden += 1;
            others.pss_kb += g.pss_kb;
            others.uss_kb += g.uss_kb;
            others.rss_kb += g.rss_kb;
            others.procs += g.procs;
        }
    }
    if hidden > 0 {
        others.name = format!("others ({hidden})");
        out.push(others);
    }
    sort_groups(&mut out);
    out
}

fn worst_ratio(row: &[f64], side: f64) -> f64 {
    let sum: f64 = row.iter().sum();
    let max = row.iter().cloned().fold(0.0_f64, f64::max);
    let min = row.iter().cloned().fold(f64::INFINITY, f64::min);
    (side * side * max / (sum * sum)).max(sum * sum / (side * side * min))
}

fn rect_from(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
    let x = x0.round() as u16;
    let y = y0.round() as u16;
    let right = x1.round() as u16;
    let bottom = y1.round() as u16;
    Rect::new(x, y, right.saturating_sub(x), bottom.saturating_sub(y))
}

/// Squarified treemap: area proportional to `values`, tiles as square as the
/// arithmetic allows. `values` must be sorted descending (as `group` returns).
pub fn squarify(values: &[u64], area: Rect) -> Vec<Rect> {
    let mut out = vec![Rect::new(area.x, area.y, 0, 0); values.len()];
    let total: u64 = values.iter().sum();
    if total == 0 || area.width < 2 || area.height < 2 {
        return out;
    }
    let scale = (area.width as f64 * area.height as f64) / total as f64;
    let (mut x, mut y) = (area.x as f64, area.y as f64);
    let (mut w, mut h) = (area.width as f64, area.height as f64);
    let mut i = 0usize;
    while i < values.len() && w >= 1.0 && h >= 1.0 {
        let side = w.min(h);
        let mut row: Vec<f64> = vec![values[i] as f64 * scale];
        let mut best = worst_ratio(&row, side);
        let mut j = i + 1;
        while j < values.len() {
            let mut cand = row.clone();
            cand.push(values[j] as f64 * scale);
            let score = worst_ratio(&cand, side);
            if score > best {
                break;
            }
            best = score;
            row = cand;
            j += 1;
        }
        let row_sum: f64 = row.iter().sum();
        if w >= h {
            let thickness = (row_sum / h).min(w);
            let mut yy = y;
            for (k, a) in row.iter().enumerate() {
                let rh = if k + 1 == row.len() {
                    y + h - yy
                } else {
                    a / thickness.max(f64::EPSILON)
                };
                out[i + k] = rect_from(x, yy, x + thickness, yy + rh);
                yy += rh;
            }
            x += thickness;
            w -= thickness;
        } else {
            let thickness = (row_sum / w).min(h);
            let mut xx = x;
            for (k, a) in row.iter().enumerate() {
                let rw = if k + 1 == row.len() {
                    x + w - xx
                } else {
                    a / thickness.max(f64::EPSILON)
                };
                out[i + k] = rect_from(xx, y, xx + rw, y + thickness);
                xx += rw;
            }
            y += thickness;
            h -= thickness;
        }
        i += j - i;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procfs::Rollup;

    fn row(pid: u32, comm: &str, rss: u64) -> ProcRow {
        ProcRow {
            pid,
            comm: comm.to_string(),
            rss_kb: rss,
            delta_kb: 0,
        }
    }

    fn info(pss: u64, uss: u64) -> PssInfo {
        PssInfo {
            rollup: Rollup {
                pss_kb: pss,
                uss_kb: uss,
                ..Default::default()
            },
            at: std::time::Instant::now(),
        }
    }

    #[test]
    fn scope_names_lose_prefix_pid_and_escapes() {
        assert_eq!(
            app_name(
                "chrome",
                Some("/user.slice/app-com.google.Chrome-391949.scope"),
                None
            ),
            "com.google.Chrome"
        );
        assert_eq!(
            app_name(
                "sing-box",
                Some("/user.slice/app-gnome-sing\\x2dbox-482443.scope"),
                None
            ),
            "gnome-sing-box"
        );
        // A scope without a numeric tail is kept as is.
        assert_eq!(app_name("x", Some("/a/app-plain.scope"), None), "plain");
    }

    #[test]
    fn non_scope_cgroups_fall_back_to_exe_then_comm() {
        assert_eq!(
            app_name(
                "node",
                Some("/user.slice/session-2.scope"),
                Some("/usr/bin/node-22")
            ),
            "node-22"
        );
        assert_eq!(
            app_name(
                "codex",
                Some("/user.slice"),
                Some("/usr/bin/codex (deleted)")
            ),
            "codex"
        );
        assert_eq!(app_name("kworker/0:1", Some("/"), None), "kworker/0:1");
    }

    #[test]
    fn processes_merge_into_one_app_across_scopes() {
        let rows = [
            row(1, "chrome", 100),
            row(2, "chrome", 200),
            row(3, "chrome", 300),
        ];
        let pss = HashMap::from([(1, info(10, 5)), (2, info(20, 6)), (3, info(30, 7))]);
        let cg = HashMap::from([
            (1, "/app-com.google.Chrome-1.scope".to_string()),
            (2, "/app-com.google.Chrome-2.scope".to_string()),
            (3, "/app-com.google.Chrome-3.scope".to_string()),
        ]);
        let groups = group(&rows, &pss, &cg, &HashMap::new());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "com.google.Chrome");
        assert_eq!(groups[0].procs, 3);
        assert_eq!(groups[0].pss_kb, 60);
        assert_eq!(groups[0].uss_kb, 18);
        assert_eq!(groups[0].pids, vec![1, 2, 3]);
    }

    #[test]
    fn groups_are_sorted_by_pss_and_ignore_missing_pss() {
        let rows = [row(1, "a", 100), row(2, "b", 900), row(3, "c", 50)];
        let pss = HashMap::from([(1, info(500, 0)), (2, info(100, 0))]);
        let groups = group(&rows, &pss, &HashMap::new(), &HashMap::new());
        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
        assert_eq!(groups[2].pss_kb, 0, "missing PSS must not be invented");
    }

    #[test]
    fn collapse_folds_the_long_tail_and_preserves_totals() {
        let mut groups: Vec<AppGroup> = (0..12)
            .map(|i| AppGroup {
                name: format!("g{i}"),
                pss_kb: if i == 0 { 5000 } else { 10 },
                uss_kb: 10,
                rss_kb: 20,
                procs: 1,
                pids: vec![i as u32],
                synthetic: false,
            })
            .collect();
        sort_groups(&mut groups);
        let total: u64 = groups.iter().map(|g| g.pss_kb).sum();
        let collapsed = collapse(groups, 0.01); // 1% of 5110 ≈ 51 kB
        assert_eq!(collapsed.len(), 2, "one big app plus others");
        assert!(collapsed[1].synthetic);
        assert_eq!(collapsed[1].name, "others (11)");
        assert_eq!(
            collapsed.iter().map(|g| g.pss_kb).sum::<u64>(),
            total,
            "collapse must not lose memory"
        );
    }

    fn overlap(a: Rect, b: Rect) -> u32 {
        let x = a.right().min(b.right()).saturating_sub(a.x.max(b.x));
        let y = a.bottom().min(b.bottom()).saturating_sub(a.y.max(b.y));
        u32::from(x) * u32::from(y)
    }

    #[test]
    fn treemap_tiles_stay_inside_and_do_not_overlap() {
        let values = [5000u64, 2500, 1200, 700, 300, 100, 50, 20, 10];
        let area = Rect::new(0, 0, 120, 40);
        let rects = squarify(&values, area);
        assert_eq!(rects.len(), values.len());
        for r in &rects {
            assert!(
                r.right() <= area.right() && r.bottom() <= area.bottom(),
                "{r:?}"
            );
        }
        for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                assert!(
                    overlap(rects[i], rects[j]) <= 1,
                    "{:?} vs {:?}",
                    rects[i],
                    rects[j]
                );
            }
        }
        // Areas track values: the biggest tile holds the biggest share.
        let tile_area = |r: Rect| u32::from(r.width) * u32::from(r.height);
        assert!(tile_area(rects[0]) > tile_area(rects[1]));
        assert!(tile_area(rects[1]) > tile_area(rects[2]));
    }

    #[test]
    fn treemap_area_is_proportional_within_a_few_percent() {
        let values = [60u64, 30, 10];
        let area = Rect::new(10, 5, 100, 50);
        let rects = squarify(&values, area);
        let total: f64 = 100.0;
        let container = 100.0 * 50.0;
        for (r, v) in rects.iter().zip(values.iter()) {
            let got = f64::from(r.width) * f64::from(r.height);
            let want = container * (*v as f64) / total;
            let err = (got - want).abs() / want;
            assert!(err < 0.15, "tile {r:?} area {got} vs {want}");
        }
    }

    #[test]
    fn treemap_handles_degenerate_input() {
        assert!(squarify(&[], Rect::new(0, 0, 10, 10)).is_empty());
        assert_eq!(squarify(&[0, 0], Rect::new(0, 0, 10, 10))[0].width, 0);
        assert_eq!(
            squarify(&[5], Rect::new(2, 3, 20, 10))[0],
            Rect::new(2, 3, 20, 10)
        );
    }
}
