//! Section grouping for palette entries.
//!
//! Groups filtered entries by their [`PaletteEntry::kind`] and exposes the
//! ordered list of non-empty groups plus their item counts, so a
//! [`gpui_component::list::ListDelegate`] can report per-section sizes and
//! render headers via [`crate::ui::components::section_header`].
//!
//! Ported from the reference launcher's `SectionManager` but made fully generic over the
//! entry's [`EntryKind`] (no launcher `ConfigModule` enum) and stripped of
//! best-match-promotion/calculator/AI special cases — those are
//! application-specific and belong in a richer delegate, not the boilerplate.

use std::collections::BTreeMap;

use crate::features::palette::items::{EntryKind, PaletteEntry};

const LOG: &str = "gpui_starter::palette::sections";

/// Ordered statistics for one group of filtered entries.
#[derive(Clone, Debug, Default)]
pub struct GroupStats {
    /// The number of filtered items in this group, in original order.
    pub count: usize,
    /// Original (unfiltered) indices belonging to this group, in order.
    pub indices: Vec<usize>,
}

/// Maintains per-group counts and ordering for a filtered entry set.
///
/// Construct once, then call [`SectionManager::rebuild`] whenever the filter
/// changes. Order of groups is determined by the first appearance of each kind
/// in the (optionally caller-supplied) preferred order, falling back to
/// first-seen order.
pub struct SectionManager<K: EntryKind> {
    /// Caller-preferred group order (kinds that should appear first, when
    /// present). Kinds not listed here keep first-seen order after them.
    preferred: Vec<K>,
    /// `kind.section_title()` -> stats. A BTreeMap keyed by the title keeps the
    /// map deterministic; ordering is rebuilt on each [`Self::rebuild`].
    groups: BTreeMap<String, GroupStats>,
    /// Ordered titles as of the last rebuild.
    ordered_titles: Vec<String>,
}

impl<K: EntryKind> SectionManager<K> {
    /// Construct with a preferred kind order. Empty `preferred` means pure
    /// first-seen ordering.
    pub fn new(preferred: Vec<K>) -> Self {
        Self {
            preferred,
            groups: BTreeMap::new(),
            ordered_titles: Vec::new(),
        }
    }

    /// Recompute grouping from `items` and the filtered `indices`.
    ///
    /// After this call, [`Self::groups_in_order`] returns the non-empty groups
    /// in display order and [`Self::total_count`] returns `indices.len()`.
    pub fn rebuild<E: PaletteEntry<Kind = K>>(&mut self, items: &[E], indices: &[usize]) {
        self.groups.clear();

        // Bucket each filtered index by its kind's section title.
        for &idx in indices {
            let Some(entry) = items.get(idx) else {
                tracing::debug!(
                    target: LOG,
                    idx,
                    "rebuild: filtered index out of range, skipping"
                );
                continue;
            };
            let title = entry.kind().section_title().to_string();
            let stats = self.groups.entry(title).or_default();
            stats.count += 1;
            stats.indices.push(idx);
        }

        // Compute display order: preferred kinds first (in preferred order),
        // then any remaining kinds in first-seen order of the filtered walk.
        let mut ordered: Vec<String> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        for kind in &self.preferred {
            let title = kind.section_title().to_string();
            if self.groups.contains_key(&title) && seen.insert(title.clone()) {
                ordered.push(title);
            }
        }
        for &idx in indices {
            let Some(entry) = items.get(idx) else {
                continue;
            };
            let title = entry.kind().section_title().to_string();
            if seen.insert(title.clone()) {
                ordered.push(title);
            }
        }

        self.ordered_titles = ordered;
    }

    /// Non-empty group titles in display order.
    pub fn groups_in_order(&self) -> &[String] {
        &self.ordered_titles
    }

    /// Stats for a group title, if present.
    pub fn group(&self, title: &str) -> Option<&GroupStats> {
        self.groups.get(title)
    }

    /// Number of non-empty groups (>= 1 for an [`gpui_component::list::ListDelegate`]).
    pub fn group_count(&self) -> usize {
        self.ordered_titles
            .len()
            .max(if self.groups.is_empty() { 0 } else { 1 })
    }

    /// Total filtered item count across all groups.
    pub fn total_count(&self) -> usize {
        self.groups.values().map(|g| g.count).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::palette::items::{KindStr, PaletteEntry};

    struct E {
        name: &'static str,
        kind: KindStr,
    }
    impl PaletteEntry for E {
        type Kind = KindStr;
        fn kind(&self) -> Self::Kind {
            self.kind.clone()
        }
        fn name(&self) -> &str {
            self.name
        }
    }

    #[test]
    fn groups_by_kind_and_preserves_first_seen_order() {
        let items = [
            E {
                name: "Home",
                kind: KindStr("Pages"),
            },
            E {
                name: "Light",
                kind: KindStr("Theme"),
            },
            E {
                name: "Form",
                kind: KindStr("Pages"),
            },
            E {
                name: "Dark",
                kind: KindStr("Theme"),
            },
        ];
        let mut sm = SectionManager::new(vec![]);
        sm.rebuild(&items, &[0, 1, 2, 3]);
        // First-seen: Pages, then Theme.
        assert_eq!(sm.groups_in_order(), ["Pages", "Theme"]);
        assert_eq!(sm.group("Pages").unwrap().count, 2);
        assert_eq!(sm.group("Theme").unwrap().count, 2);
        assert_eq!(sm.total_count(), 4);
    }

    #[test]
    fn preferred_order_is_respected() {
        let items = [
            E {
                name: "Home",
                kind: KindStr("Pages"),
            },
            E {
                name: "Light",
                kind: KindStr("Theme"),
            },
        ];
        let mut sm = SectionManager::new(vec![KindStr("Theme"), KindStr("Pages")]);
        sm.rebuild(&items, &[0, 1]);
        assert_eq!(sm.groups_in_order(), ["Theme", "Pages"]);
    }

    #[test]
    fn empty_filter_yields_no_groups() {
        let items = [E {
            name: "Home",
            kind: KindStr("Pages"),
        }];
        let mut sm = SectionManager::new(vec![]);
        sm.rebuild(&items, &[]);
        assert_eq!(sm.total_count(), 0);
    }
}
