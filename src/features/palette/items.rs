//! Trait-based palette entry model.
//!
//! [`PaletteEntry`] is the minimal, generic contract any selectable item must
//! satisfy to be scored and rendered by the palette. It deliberately knows
//! nothing about GPUI elements or application-specific command types — concrete
//! implementations (e.g. the launcher's `LauncherItem` adapter) bind those.
//!
//! Ported from the reference launcher's `ListItem` enum, but flattened into a trait so the
//! palette can be extended with new item kinds (recent files, search results,
//! AI suggestions, …) without modifying this module.

use gpui::SharedString;
use gpui_component::IconName;

/// An opaque, comparable kind tag for an entry.
///
/// Two entries share a kind when they should be grouped together by the
/// [`crate::features::palette::sections::SectionManager`]. Implementations pick
/// whatever representation is natural (an enum, a `&'static str`, …) as long as
/// equality and ordering are total.
pub trait EntryKind: PartialEq + Eq + Clone {
    /// Human-readable title used for the section header when this kind is the
    /// sole occupant of a group.
    fn section_title(&self) -> &str;
}

/// A grouping key built from a `&'static str` literal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindStr(pub &'static str);

impl EntryKind for KindStr {
    fn section_title(&self) -> &str {
        self.0
    }
}

/// The contract for a selectable, filterable palette row.
///
/// Only [`PaletteEntry::name`] and [`PaletteEntry::score_multiplier`] are
/// required; the rest have default impls. `name` is the primary search target;
/// `description`, when present, is searched as a fallback (with a penalty) and
/// shown as a secondary line.
pub trait PaletteEntry {
    /// The grouping key for sectioning.
    type Kind: EntryKind;

    /// The grouping key for this entry.
    fn kind(&self) -> Self::Kind;

    /// Primary label, also the primary fuzzy-match target.
    fn name(&self) -> &str;

    /// Optional secondary line / fallback match target.
    fn description(&self) -> Option<&str> {
        None
    }

    /// Optional icon for the row.
    fn icon(&self) -> Option<IconName> {
        None
    }

    /// Right-aligned action hint (e.g. `"Enter"`), shown when selected.
    fn action_hint(&self) -> Option<&str> {
        None
    }

    /// Per-kind score multiplier (0.0–1.0 to demote, 1.0 = neutral, >1.0 to
    /// promote). Applied on top of the global
    /// [`crate::features::palette::filter::FuzzyMatchConfig::kind_score_multiplier`].
    fn score_multiplier(&self) -> f64 {
        1.0
    }

    /// Optional, caller-defined equality hint for logging/diagnostics.
    fn debug_id(&self) -> Option<SharedString> {
        None
    }
}
