//! Pure helpers for treating `SavedConnection.group` as a `/`-delimited
//! nested path (`"OSS/Production"`) instead of a flat name. No new storage
//! or entity: a depth-1 path is exactly today's flat group name, so every
//! existing `connections.json` is already valid input here.

/// Path separator between nesting levels.
pub const GROUP_SEP: char = '/';

/// Trim, collapse repeated separators, and strip leading/trailing
/// separators. An empty result (e.g. `""`, `"/"`, `"  "`) normalizes to
/// `None` — the same "no group" meaning as today's flat model.
pub fn normalize_group_path(raw: &str) -> Option<String> {
    let normalized = raw
        .split(GROUP_SEP)
        .map(str::trim)
        .filter(|seg| !seg.is_empty())
        .collect::<Vec<_>>()
        .join(&GROUP_SEP.to_string());
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// Parent path one level up, or `None` if `path` is already top-level.
pub fn group_parent(path: &str) -> Option<&str> {
    path.rsplit_once(GROUP_SEP).map(|(parent, _)| parent)
}

/// The final segment of a path (its display label at its own depth).
pub fn group_leaf(path: &str) -> &str {
    path.rsplit_once(GROUP_SEP)
        .map(|(_, leaf)| leaf)
        .unwrap_or(path)
}

/// Every proper ancestor of `path`, shallowest first: `"A/B/C"` yields
/// `"A"`, `"A/B"` (not `"A/B/C"` itself).
pub fn group_ancestors(path: &str) -> impl Iterator<Item = &str> {
    path.match_indices(GROUP_SEP).map(move |(i, _)| &path[..i])
}

/// True if `path` is `ancestor` itself or nested under it.
pub fn is_descendant(path: &str, ancestor: &str) -> bool {
    path == ancestor
        || path
            .strip_prefix(ancestor)
            .is_some_and(|rest| rest.starts_with(GROUP_SEP))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_empty_and_whitespace() {
        assert_eq!(normalize_group_path(""), None);
        assert_eq!(normalize_group_path("   "), None);
        assert_eq!(normalize_group_path("/"), None);
        assert_eq!(normalize_group_path("///"), None);
    }

    #[test]
    fn normalize_trims_and_collapses() {
        assert_eq!(normalize_group_path("OSS"), Some("OSS".into()));
        assert_eq!(
            normalize_group_path("/OSS/Production/"),
            Some("OSS/Production".into())
        );
        assert_eq!(
            normalize_group_path("OSS// Production "),
            Some("OSS/Production".into())
        );
        assert_eq!(
            normalize_group_path(" OSS / Production / DB "),
            Some("OSS/Production/DB".into())
        );
    }

    #[test]
    fn parent_and_leaf() {
        assert_eq!(group_parent("OSS"), None);
        assert_eq!(group_parent("OSS/Production"), Some("OSS"));
        assert_eq!(group_parent("OSS/Production/DB"), Some("OSS/Production"));

        assert_eq!(group_leaf("OSS"), "OSS");
        assert_eq!(group_leaf("OSS/Production"), "Production");
        assert_eq!(group_leaf("OSS/Production/DB"), "DB");
    }

    #[test]
    fn ancestors_are_shallowest_first_and_exclude_self() {
        assert_eq!(
            group_ancestors("OSS").collect::<Vec<_>>(),
            Vec::<&str>::new()
        );
        assert_eq!(
            group_ancestors("OSS/Production/DB").collect::<Vec<_>>(),
            vec!["OSS", "OSS/Production"]
        );
    }

    #[test]
    fn descendant_check() {
        assert!(is_descendant("OSS", "OSS"));
        assert!(is_descendant("OSS/Production", "OSS"));
        assert!(is_descendant("OSS/Production/DB", "OSS"));
        assert!(is_descendant("OSS/Production/DB", "OSS/Production"));
        assert!(!is_descendant("OSS", "OSS/Production"));
        assert!(!is_descendant("OSSLegacy", "OSS"));
        assert!(!is_descendant("Other", "OSS"));
    }
}
