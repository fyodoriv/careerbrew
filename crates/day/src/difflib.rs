//! A faithful port of CPython's `difflib.SequenceMatcher` with `isjunk=None`.
//!
//! PM learning uses `ratio()` on code-point strings (autojunk on) and
//! `get_opcodes()` on line lists (autojunk off). The popular-element heuristic
//! applies only when the second sequence has at least 200 elements.

use std::collections::HashMap;
use std::hash::Hash;

/// One matching block: `a[a..a+size] == b[b..b+size]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Match {
    pub a: usize,
    pub b: usize,
    pub size: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opcode {
    pub tag: &'static str,
    pub i1: usize,
    pub i2: usize,
    pub j1: usize,
    pub j2: usize,
}

pub struct SequenceMatcher<'a, T: Eq + Hash> {
    a: &'a [T],
    b: &'a [T],
    b2j: HashMap<&'a T, Vec<usize>>,
}

impl<'a, T: Eq + Hash> SequenceMatcher<'a, T> {
    pub fn new(a: &'a [T], b: &'a [T], autojunk: bool) -> Self {
        let mut b2j: HashMap<&'a T, Vec<usize>> = HashMap::new();
        for (index, item) in b.iter().enumerate() {
            b2j.entry(item).or_default().push(index);
        }
        let n = b.len();
        if autojunk && n >= 200 {
            let ntest = n / 100 + 1;
            b2j.retain(|_, indexes| indexes.len() <= ntest);
        }
        Self { a, b, b2j }
    }

    fn find_longest_match(&self, alo: usize, ahi: usize, blo: usize, bhi: usize) -> Match {
        let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0usize);
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        for i in alo..ahi {
            let mut next: HashMap<usize, usize> = HashMap::new();
            if let Some(indexes) = self.b2j.get(&self.a[i]) {
                for &j in indexes {
                    if j < blo {
                        continue;
                    }
                    if j >= bhi {
                        break;
                    }
                    let k = j
                        .checked_sub(1)
                        .and_then(|prev| j2len.get(&prev))
                        .copied()
                        .unwrap_or(0)
                        + 1;
                    next.insert(j, k);
                    if k > bestsize {
                        besti = i + 1 - k;
                        bestj = j + 1 - k;
                        bestsize = k;
                    }
                }
            }
            j2len = next;
        }
        // No junk set exists (isjunk is None), so popular elements extend the
        // match exactly like CPython's "not isbjunk" loops.
        while besti > alo && bestj > blo && self.a[besti - 1] == self.b[bestj - 1] {
            besti -= 1;
            bestj -= 1;
            bestsize += 1;
        }
        while besti + bestsize < ahi
            && bestj + bestsize < bhi
            && self.a[besti + bestsize] == self.b[bestj + bestsize]
        {
            bestsize += 1;
        }
        Match {
            a: besti,
            b: bestj,
            size: bestsize,
        }
    }

    pub fn matching_blocks(&self) -> Vec<Match> {
        let (la, lb) = (self.a.len(), self.b.len());
        let mut queue = vec![(0, la, 0, lb)];
        let mut blocks = Vec::new();
        while let Some((alo, ahi, blo, bhi)) = queue.pop() {
            let found = self.find_longest_match(alo, ahi, blo, bhi);
            let (i, j, k) = (found.a, found.b, found.size);
            if k > 0 {
                blocks.push(found);
                if alo < i && blo < j {
                    queue.push((alo, i, blo, j));
                }
                if i + k < ahi && j + k < bhi {
                    queue.push((i + k, ahi, j + k, bhi));
                }
            }
        }
        blocks.sort();
        let (mut i1, mut j1, mut k1) = (0, 0, 0);
        let mut out = Vec::new();
        for block in blocks {
            if i1 + k1 == block.a && j1 + k1 == block.b {
                k1 += block.size;
            } else {
                if k1 > 0 {
                    out.push(Match {
                        a: i1,
                        b: j1,
                        size: k1,
                    });
                }
                (i1, j1, k1) = (block.a, block.b, block.size);
            }
        }
        if k1 > 0 {
            out.push(Match {
                a: i1,
                b: j1,
                size: k1,
            });
        }
        out.push(Match {
            a: la,
            b: lb,
            size: 0,
        });
        out
    }

    pub fn ratio(&self) -> f64 {
        let matches: usize = self.matching_blocks().iter().map(|m| m.size).sum();
        let total = self.a.len() + self.b.len();
        if total == 0 {
            1.0
        } else {
            2.0 * matches as f64 / total as f64
        }
    }

    pub fn opcodes(&self) -> Vec<Opcode> {
        let (mut i, mut j) = (0, 0);
        let mut out = Vec::new();
        for block in self.matching_blocks() {
            let tag = if i < block.a && j < block.b {
                "replace"
            } else if i < block.a {
                "delete"
            } else if j < block.b {
                "insert"
            } else {
                ""
            };
            if !tag.is_empty() {
                out.push(Opcode {
                    tag,
                    i1: i,
                    i2: block.a,
                    j1: j,
                    j2: block.b,
                });
            }
            i = block.a + block.size;
            j = block.b + block.size;
            if block.size > 0 {
                out.push(Opcode {
                    tag: "equal",
                    i1: block.a,
                    i2: i,
                    j1: block.b,
                    j2: j,
                });
            }
        }
        out
    }
}

/// `difflib.SequenceMatcher(None, a, b).ratio()` over code points.
pub fn ratio(a: &str, b: &str) -> f64 {
    let a = a.chars().collect::<Vec<_>>();
    let b = b.chars().collect::<Vec<_>>();
    SequenceMatcher::new(&a, &b, true).ratio()
}

/// `get_matching_blocks()` over code points, autojunk on.
pub fn blocks(a: &str, b: &str) -> Vec<Match> {
    let a = a.chars().collect::<Vec<_>>();
    let b = b.chars().collect::<Vec<_>>();
    SequenceMatcher::new(&a, &b, true).matching_blocks()
}

/// `SequenceMatcher(a=..., b=..., autojunk=False).get_opcodes()` over lines.
pub fn line_opcodes(a: &[String], b: &[String]) -> Vec<Opcode> {
    SequenceMatcher::new(a, b, false).opcodes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_cpython_reference_ratios() {
        assert_eq!(ratio("", ""), 1.0);
        assert_eq!(ratio("abcd", "bcda"), 0.75);
        assert_eq!(
            ratio("Billing: shipped invoices", "Billing: shipped the invoices"),
            0.9259259259259259
        );
    }
}
