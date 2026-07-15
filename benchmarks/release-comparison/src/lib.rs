#![forbid(unsafe_code)]

use std::collections::BTreeSet;

pub const WORKLOAD_MANIFEST: &str = include_str!("../workloads.tsv");

pub fn parameters(group: &str) -> Vec<usize> {
    let selected: Vec<_> = workloads()
        .into_iter()
        .filter_map(|(row_group, parameter)| (row_group == group).then_some(parameter))
        .collect();
    assert!(!selected.is_empty(), "workload group is absent: {group}");
    selected
}

pub fn workloads() -> Vec<(&'static str, usize)> {
    workloads_from_manifest(WORKLOAD_MANIFEST, "safe-bump-release-workloads-v1")
}

fn workloads_from_manifest<'a>(
    manifest: &'a str,
    expected_header: &str,
) -> Vec<(&'a str, usize)> {
    let mut lines = manifest.lines();
    assert_eq!(
        lines.next(),
        Some(expected_header),
        "invalid release workload manifest header"
    );

    let mut seen = BTreeSet::new();
    let mut workloads = Vec::new();
    for (offset, line) in lines.enumerate() {
        assert!(
            !line.is_empty(),
            "empty workload row at line {}",
            offset + 2
        );
        let (row_group, parameter) = line
            .split_once('\t')
            .unwrap_or_else(|| panic!("malformed workload row at line {}", offset + 2));
        assert!(
            !parameter.contains('\t') && !row_group.is_empty() && !parameter.is_empty(),
            "malformed workload row at line {}",
            offset + 2
        );
        assert!(
            seen.insert((row_group, parameter)),
            "duplicate workload row at line {}",
            offset + 2
        );
        let value = parameter
            .parse::<usize>()
            .unwrap_or_else(|_| panic!("non-numeric workload parameter at line {}", offset + 2));
        workloads.push((row_group, value));
    }
    assert!(!workloads.is_empty(), "release workload manifest is empty");
    workloads
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_manifest_is_closed_unique_and_complete() {
        let groups = [
            ("release/allocation", vec![64, 1_024, 65_536]),
            ("release/validated_lookup", vec![64, 1_024, 65_536]),
            ("release/iteration", vec![64, 1_024, 65_536]),
            ("release/speculative_rollback", vec![1, 64, 1_024]),
            ("release/shared_concurrent_allocation", vec![1, 2, 4, 8]),
        ];
        for (group, expected) in groups {
            assert_eq!(parameters(group), expected, "group {group}");
        }
        assert_eq!(workloads().len(), 16);
        assert_eq!(WORKLOAD_MANIFEST.lines().count(), 17);
    }

    #[test]
    #[should_panic(expected = "empty workload row at line 2")]
    fn empty_manifest_row_is_rejected() {
        workloads_from_manifest(
            "safe-bump-release-workloads-v1\n\nrelease/allocation\t64\n",
            "safe-bump-release-workloads-v1",
        );
    }

    #[test]
    #[should_panic(expected = "malformed workload row at line 2")]
    fn comment_manifest_row_is_rejected() {
        workloads_from_manifest(
            "safe-bump-release-workloads-v1\n# hidden row\n",
            "safe-bump-release-workloads-v1",
        );
    }

    #[test]
    #[should_panic(expected = "duplicate workload row at line 3")]
    fn duplicate_manifest_row_is_rejected() {
        workloads_from_manifest(
            "safe-bump-release-workloads-v1\nrelease/allocation\t64\nrelease/allocation\t64\n",
            "safe-bump-release-workloads-v1",
        );
    }

    #[test]
    #[should_panic(expected = "release workload manifest is empty")]
    fn empty_manifest_is_rejected() {
        workloads_from_manifest(
            "safe-bump-release-workloads-v1\n",
            "safe-bump-release-workloads-v1",
        );
    }
}
