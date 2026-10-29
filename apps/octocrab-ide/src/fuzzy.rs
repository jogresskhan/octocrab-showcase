//! Fuzzy subsequence scoring (quick-open + command palette).
//!
//! Simple, dependency-free, fast enough to score thousands of paths per frame.

/// Score `needle` against `hay`; `None` when the needle is not a
/// (case-insensitive, in-order) subsequence. Higher is better.
pub fn score(needle: &str, hay: &str) -> Option<i64> {
    let n: Vec<char> = needle
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if n.is_empty() {
        return Some(-(hay.len() as i64) / 2);
    }
    let h: Vec<char> = hay.chars().map(|c| c.to_ascii_lowercase()).collect();

    let mut s: i64 = 0;
    let mut hi = 0usize;
    let mut prev: Option<usize> = None;

    for &c in &n {
        let mut found = None;
        let mut j = hi;
        while j < h.len() {
            if h[j] == c {
                found = Some(j);
                break;
            }
            j += 1;
        }
        let idx = found?;
        s += 10;
        if prev == Some(idx.wrapping_sub(1)) {
            s += 12; // consecutive match
        }
        if idx == 0 || matches!(h[idx - 1], '/' | '_' | '-' | '.' | ' ') {
            s += 8; // boundary match
        }
        s -= (idx - hi) as i64; // skipped-gap penalty
        prev = Some(idx);
        hi = idx + 1;
    }

    Some(s - hay.len() as i64 / 2) // prefer shorter paths
}

/// Rank items by fuzzy score, best first.
pub fn rank(needle: &str, items: impl Iterator<Item = String>) -> Vec<(i64, String)> {
    let mut v: Vec<(i64, String)> = items
        .filter_map(|s| score(needle, &s).map(|sc| (sc, s)))
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    v
}
