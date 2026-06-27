//! Fuzzy scoring and filtering for palette items.
//!
//! Wraps [`fuzzy_matcher::skim::SkimMatcherV2`] with a JSON-configurable
//! [`FuzzyMatchConfig`] that exposes tuning knobs (bonuses for exact / prefix /
//! word-prefix / boundary-contiguity matches, a description-only penalty, and a
//! generic kind multiplier) so application configuration can shape ranking
//! without code changes.
//!
//! Ported from the reference launcher's `item_filter.rs` but rebound to gpui-starter's
//! [`PaletteEntry`] trait (see [`crate::features::palette::items`]) instead of
//! the reference launcher's `ListItem` enum, and with all configuration expressed as plain
//! serde-serialisable fields rather than launcher-specific config types.

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use serde::{Deserialize, Serialize};

use crate::features::palette::items::PaletteEntry;

const LOG: &str = "gpui_starter::palette::filter";

/// A scored filter hit: the index into the original item slice plus its score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilteredItem {
    /// Index into the source items vector.
    pub index: usize,
    /// Fuzzy match score (higher is better). May be negative.
    pub score: i64,
}

/// JSON-configurable tuning for [`ItemFilter`] scoring.
///
/// Every field has a sensible default and is `Serialize`+`Deserialize` so the
/// whole struct can be dropped into a config file (e.g. a `palette.toml`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FuzzyMatchConfig {
    /// Bonus added when an item's name equals the query exactly.
    pub exact_match_bonus: i64,
    /// Bonus added when the name starts with the query.
    pub prefix_match_bonus: i64,
    /// Bonus added when the query matches the start of any whitespace-delimited
    /// word in the name.
    pub word_prefix_bonus: i64,
    /// Maximum bonus for contiguous (adjacent) matched characters; scaled by the
    /// observed contiguity ratio.
    pub contiguity_bonus: i64,
    /// Multiplier applied to a match found only in the description (0.0–1.0
    /// demotes description-only hits below name hits).
    pub description_penalty: f64,
    /// Generic multiplier applied per item kind (returned by
    /// [`PaletteEntry::score_multiplier`]); lets noisy kinds be demoted.
    pub kind_score_multiplier: f64,
}

impl Default for FuzzyMatchConfig {
    fn default() -> Self {
        Self {
            exact_match_bonus: 1000,
            prefix_match_bonus: 500,
            word_prefix_bonus: 250,
            contiguity_bonus: 100,
            description_penalty: 0.6,
            // Identity by default: PaletteEntry::score_multiplier carries any
            // per-kind demotion.
            kind_score_multiplier: 1.0,
        }
    }
}

/// Fuzzy filter over anything implementing [`PaletteEntry`].
pub struct ItemFilter {
    matcher: SkimMatcherV2,
    /// Public so callers (e.g. a settings UI) can read/write the active tuning.
    pub config: FuzzyMatchConfig,
}

impl Default for ItemFilter {
    fn default() -> Self {
        Self::new(FuzzyMatchConfig::default())
    }
}

impl ItemFilter {
    /// Construct a filter with explicit scoring configuration.
    pub fn new(config: FuzzyMatchConfig) -> Self {
        Self {
            matcher: SkimMatcherV2::default(),
            config,
        }
    }

    /// Return just the matching indices, sorted by descending score.
    ///
    /// Convenience wrapper around [`ItemFilter::filter_with_scores`].
    pub fn filter_indices<E: PaletteEntry>(&self, items: &[E], query: &str) -> Vec<usize> {
        self.filter_with_scores(items, query)
            .into_iter()
            .map(|f| f.index)
            .collect()
    }

    /// Score and sort all items against `query`.
    ///
    /// - Empty query: every item is returned with score `0`, in original order.
    /// - Non-empty query: only items that fuzzy-match are returned, sorted by
    ///   descending score (ties broken by original index for stable ordering).
    pub fn filter_with_scores<E: PaletteEntry>(
        &self,
        items: &[E],
        query: &str,
    ) -> Vec<FilteredItem> {
        if query.is_empty() {
            return (0..items.len())
                .map(|index| FilteredItem { index, score: 0 })
                .collect();
        }

        let mut scored: Vec<FilteredItem> = items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| {
                let score = self.score_entry(item, query)?;
                Some(FilteredItem { index: idx, score })
            })
            .collect();

        // Higher score first; stable on ties by original index.
        scored.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.index.cmp(&b.index)));
        scored
    }

    /// Score a single entry, preferring name matches and falling back to the
    /// description with a penalty.
    fn score_entry<E: PaletteEntry>(&self, item: &E, query: &str) -> Option<i64> {
        let name = item.name();

        if let Some(score) = self.score_text(name, query, false) {
            return Some(self.apply_kind_multiplier(score, item));
        }

        if let Some(desc) = item.description() {
            if let Some(score) = self.score_text(desc, query, true) {
                return Some(self.apply_kind_multiplier(score, item));
            }
        }

        None
    }

    /// Core text scoring with query-normalisation, bonus application, and the
    /// description penalty.
    ///
    /// Returns `None` when the matcher reports no hit for any normalisation.
    fn score_text(&self, text: &str, query: &str, is_description: bool) -> Option<i64> {
        let query_lower = query.to_lowercase();
        let text_lower = text.to_lowercase();

        // Try the original query first.
        let matched = self.matcher.fuzzy_indices(text, query).or_else(|| {
            // Normalise whitespace: "foo bar" -> "foobar" and "foo-bar", which
            // lets "counter strike" match "Counter-Strike".
            if !query.contains(' ') {
                return None;
            }
            let no_spaces: String = query.chars().filter(|c| *c != ' ').collect();
            if let Some(hit) = self.matcher.fuzzy_indices(text, &no_spaces) {
                return Some(hit);
            }
            let with_hyphens = query.replace(' ', "-");
            self.matcher.fuzzy_indices(text, &with_hyphens)
        });

        let (base_score, indices) = matched?;
        let mut score = base_score;

        // Bonuses apply only to name matches, not description matches.
        if !is_description {
            if text_lower == query_lower {
                score += self.config.exact_match_bonus;
            } else if text_lower.starts_with(&query_lower) {
                score += self.config.prefix_match_bonus;
            } else if Self::matches_word_start(text, &query_lower) {
                score += self.config.word_prefix_bonus;
            }
        }

        score += self.contiguity_bonus(&indices);

        if is_description {
            score = (score as f64 * self.config.description_penalty) as i64;
        }

        Some(score)
    }

    /// Scale a score by the entry's own kind multiplier and the global one.
    fn apply_kind_multiplier<E: PaletteEntry>(&self, score: i64, item: &E) -> i64 {
        let mult = item.score_multiplier() * self.config.kind_score_multiplier;
        if (mult - 1.0).abs() < f64::EPSILON {
            return score;
        }
        let scaled = (score as f64 * mult) as i64;
        if scaled == 0 && score != 0 {
            tracing::trace!(
                target: LOG,
                score,
                multiplier = mult,
                "kind multiplier reduced score to zero"
            );
        }
        scaled
    }

    /// Linearly scale the contiguity bonus by how many matched-character pairs
    /// are adjacent. Single-character matches get the full bonus.
    fn contiguity_bonus(&self, indices: &[usize]) -> i64 {
        if indices.len() <= 1 {
            return self.config.contiguity_bonus;
        }
        let adjacent = indices.windows(2).filter(|w| w[1] == w[0] + 1).count();
        let ratio = adjacent as f64 / (indices.len() - 1) as f64;
        (ratio * self.config.contiguity_bonus as f64) as i64
    }

    /// True when `query_lower` prefixes any whitespace-delimited word in `text`.
    fn matches_word_start(text: &str, query_lower: &str) -> bool {
        text.split_whitespace()
            .any(|word| word.to_lowercase().starts_with(query_lower))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::palette::items::{KindStr, PaletteEntry};

    // Minimal test entry: name + optional description + neutral multiplier.
    struct Entry {
        name: &'static str,
        desc: Option<&'static str>,
    }
    impl PaletteEntry for Entry {
        type Kind = KindStr;
        fn kind(&self) -> Self::Kind {
            KindStr("Test")
        }
        fn name(&self) -> &str {
            self.name
        }
        fn description(&self) -> Option<&str> {
            self.desc
        }
        fn score_multiplier(&self) -> f64 {
            1.0
        }
    }

    #[test]
    fn empty_query_returns_all() {
        let f = ItemFilter::default();
        let items = [
            Entry {
                name: "Firefox",
                desc: None,
            },
            Entry {
                name: "Chrome",
                desc: None,
            },
        ];
        let r = f.filter_indices(&items, "");
        assert_eq!(r, vec![0, 1]);
    }

    #[test]
    fn filters_by_name_substring() {
        let f = ItemFilter::default();
        let items = [
            Entry {
                name: "Firefox",
                desc: None,
            },
            Entry {
                name: "Chrome",
                desc: None,
            },
        ];
        let r = f.filter_indices(&items, "fire");
        assert_eq!(r, vec![0]);
    }

    #[test]
    fn exact_name_beats_description_only_match() {
        let f = ItemFilter::default();
        let items = [
            Entry {
                name: "Browser",
                desc: None,
            }, // name match
            Entry {
                name: "Editor",
                desc: Some("a browser for files"),
            }, // desc only
        ];
        let scored = f.filter_with_scores(&items, "browser");
        assert_eq!(scored[0].index, 0);
        assert!(scored[0].score > scored[1].score);
    }

    #[test]
    fn prefix_beats_infix() {
        let f = ItemFilter::default();
        let items = [
            Entry {
                name: "Firefox",
                desc: None,
            }, // prefix
            Entry {
                name: "Waterfox",
                desc: None,
            }, // infix
        ];
        let r = f.filter_indices(&items, "fire");
        assert_eq!(r[0], 0);
    }

    #[test]
    fn space_query_matches_hyphenated_name() {
        let f = ItemFilter::default();
        let items = [Entry {
            name: "Counter-Strike",
            desc: None,
        }];
        let r = f.filter_indices(&items, "counter strike");
        assert_eq!(r, vec![0]);
    }

    #[test]
    fn no_match_returns_empty() {
        let f = ItemFilter::default();
        let items = [Entry {
            name: "Firefox",
            desc: None,
        }];
        let r = f.filter_indices(&items, "zzz");
        assert!(r.is_empty());
    }
}
