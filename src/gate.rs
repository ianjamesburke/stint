use std::collections::HashSet;

use crate::schema::{BlockedByRef, Gate, Task};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockerSource {
    Direct,
    Gate(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockerInfo {
    pub reference: BlockedByRef,
    pub source: BlockerSource,
}

pub fn effective_blockers(task: &Task, gates: &[Gate]) -> Vec<BlockedByRef> {
    effective_blocker_infos(task, gates)
        .into_iter()
        .map(|blocker| blocker.reference)
        .collect()
}

pub fn effective_blocker_infos(task: &Task, gates: &[Gate]) -> Vec<BlockerInfo> {
    let mut blockers: Vec<BlockerInfo> = task
        .frontmatter
        .blocked_by
        .iter()
        .cloned()
        .map(|reference| BlockerInfo {
            reference,
            source: BlockerSource::Direct,
        })
        .collect();
    for gate in gates {
        if gate_applies_to_task(gate, task) {
            blockers.extend(
                gate.blocked_by
                    .iter()
                    .cloned()
                    .map(|reference| BlockerInfo {
                        reference,
                        source: BlockerSource::Gate(gate.id.clone()),
                    }),
            );
        }
    }
    dedupe_blockers(blockers)
}

pub fn active_blockers(task: &Task, gates: &[Gate], done_ids: &HashSet<&str>) -> Vec<BlockedByRef> {
    active_blocker_infos(task, gates, done_ids)
        .into_iter()
        .map(|blocker| blocker.reference)
        .collect()
}

pub fn active_blocker_infos(
    task: &Task,
    gates: &[Gate],
    done_ids: &HashSet<&str>,
) -> Vec<BlockerInfo> {
    effective_blocker_infos(task, gates)
        .into_iter()
        .filter(|blocker| match &blocker.reference {
            BlockedByRef::LocalTask(id) => !done_ids.contains(id.as_str()),
            _ => true,
        })
        .collect()
}

pub fn gate_applies_to_task(gate: &Gate, task: &Task) -> bool {
    if gate.applies_to.tags.is_empty() {
        return false;
    }
    let task_tags: HashSet<&str> = task.frontmatter.tags.iter().map(String::as_str).collect();
    gate.applies_to
        .tags
        .iter()
        .any(|tag| task_tags.contains(tag.as_str()))
}

fn dedupe_blockers(blockers: Vec<BlockerInfo>) -> Vec<BlockerInfo> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for blocker in blockers {
        if seen.insert(blocker.reference.to_string()) {
            deduped.push(blocker);
        }
    }
    deduped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{parse_gate, parse_task};

    fn task(extra: &str) -> Task {
        let content = format!("---\nid: \"0001\"\ntitle: \"T\"\nstatus: todo\n{extra}\n---\n");
        parse_task(&content, "0001-t.md").unwrap()
    }

    fn gate() -> Gate {
        parse_gate(
            "---\nid: v2-after-v1\napplies_to:\n  tags: [\"v2\"]\nblocked_by:\n  - 0030\n---\n",
            "v2-after-v1.md",
        )
        .unwrap()
    }

    #[test]
    fn tag_gate_adds_inherited_blocker() {
        let task = task("tags: [\"v2\"]");
        let blockers = effective_blockers(&task, &[gate()]);
        assert_eq!(blockers, vec![BlockedByRef::LocalTask("0030".to_owned())]);
    }

    #[test]
    fn tag_gate_marks_blocker_source() {
        let task = task("blocked_by: [\"0002\"]\ntags: [\"v2\"]");
        let blockers = effective_blocker_infos(&task, &[gate()]);
        assert_eq!(blockers[0].source, BlockerSource::Direct);
        assert_eq!(
            blockers[1].source,
            BlockerSource::Gate("v2-after-v1".to_owned())
        );
    }

    #[test]
    fn unmatched_gate_does_not_apply() {
        let task = task("tags: [\"v1\"]");
        assert!(effective_blockers(&task, &[gate()]).is_empty());
    }

    #[test]
    fn active_blockers_ignores_done_local_blockers() {
        let task = task("tags: [\"v2\"]");
        let done_ids = HashSet::from(["0030"]);
        assert!(active_blockers(&task, &[gate()], &done_ids).is_empty());
    }
}
