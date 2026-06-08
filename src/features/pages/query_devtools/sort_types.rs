// ---------------------------------------------------------------------------
// Sort mode
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuerySort {
    ByKey,
    ByStatus,
    ByCacheHits,
}
