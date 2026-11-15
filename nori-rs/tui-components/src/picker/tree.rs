//! Indent guides for picker rows that form a tree.

/// Guides for each row, given the depth of every visible row in order.
/// Each entry is the prefix for the row's first line and the prefix for its
/// description line. Top-level rows get no guides.
pub(crate) fn tree_guides(depths: &[i32]) -> Vec<(String, String)> {
    // A filter can hide a row's parent. Each row nests at most one level below
    // the row above it, so guides never hang from a row that is not shown.
    let mut previous = -1;
    let depths: Vec<i32> = depths
        .iter()
        .map(|depth| {
            previous = (*depth).clamp(0, previous + 1);
            previous
        })
        .collect();
    let depths = depths.as_slice();
    (0..depths.len())
        .map(|row| {
            let depth = depths[row];
            if depth <= 0 {
                return (String::new(), String::new());
            }
            let mut first = String::new();
            let mut continuation = String::new();
            for level in 1..depth {
                let guide = if sibling_follows(depths, row, level) {
                    "│  "
                } else {
                    "   "
                };
                first.push_str(guide);
                continuation.push_str(guide);
            }
            if sibling_follows(depths, row, depth) {
                first.push_str("├─ ");
                continuation.push_str("│  ");
            } else {
                first.push_str("└─ ");
                continuation.push_str("   ");
            }
            (first, continuation)
        })
        .collect()
}

/// Whether a later row at `level` continues the branch that `row` sits in,
/// before the tree climbs above `level`.
fn sibling_follows(depths: &[i32], row: usize, level: i32) -> bool {
    depths[row + 1..]
        .iter()
        .find(|depth| **depth <= level)
        .is_some_and(|depth| *depth == level)
}
