//! Process-local scheduling. The caller supplies monotonic time and an immutable target.
//! No worker, persistence, or fallback submission path lives here.
use std::collections::BTreeSet;
use std::time::Duration;
use std::time::Instant;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Schedule {
    Every(Duration),
    Once(Instant),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Ready,
    Pending {
        attempt: u64,
        terminal_turns: BTreeSet<String>,
    },
    Running {
        turn_id: String,
        cancelling: bool,
    },
    Finished(String),
    Unknown(String),
    BlockedUnsupported(String),
    BlockedConnection(String),
}

#[derive(Debug)]
pub(crate) struct Job<T> {
    pub(crate) id: u64,
    pub(crate) target: T,
    pub(crate) prompt: String,
    pub(crate) phase: Phase,
    pub(crate) paused: bool,
    pub(crate) removed: bool,
    pub(crate) skipped: u64,
    pub(crate) next_due: Option<Instant>,
    pub(crate) interval: Option<Duration>,
    attempt: u64,
}

impl<T> Job<T> {
    pub(crate) fn holds_target(&self) -> bool {
        (!self.removed
            && !matches!(
                self.phase,
                Phase::Finished(_) | Phase::BlockedUnsupported(_) | Phase::BlockedConnection(_)
            ))
            || matches!(
                self.phase,
                Phase::Pending { .. } | Phase::Running { .. } | Phase::Unknown(_)
            )
    }
}

#[derive(Debug)]
pub(crate) enum Submission {
    Started(String),
    Busy,
    NotStarted(String),
    Unknown(String),
}

pub(crate) struct Dispatch<T> {
    pub(crate) id: u64,
    pub(crate) attempt: u64,
    pub(crate) target: T,
    pub(crate) prompt: String,
}

pub(crate) struct Scheduler<T> {
    pub(crate) jobs: Vec<Job<T>>,
    next_id: u64,
}

impl<T> Default for Scheduler<T> {
    fn default() -> Self {
        Self {
            jobs: Vec::new(),
            next_id: 1,
        }
    }
}

impl<T: Clone + PartialEq> Scheduler<T> {
    pub(crate) fn add(
        &mut self,
        target: T,
        prompt: String,
        schedule: Schedule,
        now: Instant,
    ) -> Result<u64, String> {
        if prompt.trim().is_empty() {
            return Err("예약 프롬프트가 비어 있습니다.".into());
        }
        let (interval, next_due) = match schedule {
            Schedule::Every(interval) if !interval.is_zero() => {
                (Some(interval), now.checked_add(interval))
            }
            Schedule::Every(_) => return Err("예약 간격은 0보다 커야 합니다.".into()),
            Schedule::Once(deadline) => (None, Some(deadline)),
        };
        let next_due = next_due.ok_or("예약 간격이 너무 큽니다.")?;
        let id = self.next_id;
        self.next_id = id.checked_add(1).ok_or("예약 ID를 더 만들 수 없습니다.")?;
        self.jobs.push(Job {
            id,
            target,
            prompt,
            phase: Phase::Ready,
            paused: false,
            removed: false,
            skipped: 0,
            next_due: Some(next_due),
            interval,
            attempt: 0,
        });
        Ok(id)
    }

    pub(crate) fn job_mut(&mut self, id: u64) -> Result<&mut Job<T>, String> {
        self.jobs
            .iter_mut()
            .find(|job| job.id == id)
            .ok_or_else(|| format!("예약 {id}를 찾을 수 없습니다."))
    }

    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.jobs
            .iter()
            .filter(|job| {
                !job.paused
                    && !job.removed
                    && !matches!(
                        job.phase,
                        Phase::Unknown(_)
                            | Phase::BlockedUnsupported(_)
                            | Phase::BlockedConnection(_)
                    )
            })
            .filter_map(|job| job.next_due)
            .min()
    }

    pub(crate) fn due(&mut self, now: Instant) -> Vec<Dispatch<T>> {
        let mut dispatches = Vec::new();
        for job in &mut self.jobs {
            if job.paused
                || job.removed
                || matches!(
                    job.phase,
                    Phase::Unknown(_) | Phase::BlockedUnsupported(_) | Phase::BlockedConnection(_)
                )
                || job.next_due.is_none_or(|due| due > now)
            {
                continue;
            }
            // Missed slots are coalesced: at most one admission attempt, then a future slot.
            job.next_due = job.interval.and_then(|interval| now.checked_add(interval));
            if !matches!(job.phase, Phase::Ready) {
                job.skipped = job.skipped.saturating_add(1);
                continue;
            }
            job.attempt += 1;
            job.phase = Phase::Pending {
                attempt: job.attempt,
                terminal_turns: BTreeSet::new(),
            };
            dispatches.push(Dispatch {
                id: job.id,
                attempt: job.attempt,
                target: job.target.clone(),
                prompt: job.prompt.clone(),
            });
        }
        dispatches
    }

    pub(crate) fn submitted(&mut self, id: u64, attempt: u64, submission: Submission) -> bool {
        let Ok(job) = self.job_mut(id) else {
            return false;
        };
        let Phase::Pending {
            attempt: expected,
            terminal_turns,
        } = &job.phase
        else {
            return false;
        };
        if *expected != attempt {
            return false;
        }
        job.phase = match submission {
            Submission::Started(turn_id) if terminal_turns.contains(&turn_id) => {
                Self::after_terminal(job.interval, "종료")
            }
            Submission::Started(turn_id) => Phase::Running {
                turn_id,
                cancelling: false,
            },
            Submission::Busy => {
                job.skipped = job.skipped.saturating_add(1);
                Self::after_terminal(job.interval, "대상 busy: 건너뜀")
            }
            Submission::NotStarted(reason) => {
                job.paused = true;
                Phase::Finished(format!("미입장: {reason}"))
            }
            Submission::Unknown(reason) => Phase::Unknown(reason),
        };
        self.prune_removed();
        true
    }

    fn after_terminal(interval: Option<Duration>, outcome: &str) -> Phase {
        if interval.is_some() {
            Phase::Ready
        } else {
            Phase::Finished(outcome.into())
        }
    }

    pub(crate) fn terminal(&mut self, target: &T, turn_id: &str) {
        for job in self.jobs.iter_mut().filter(|job| &job.target == target) {
            match &mut job.phase {
                Phase::Pending { terminal_turns, .. } => {
                    terminal_turns.insert(turn_id.to_string());
                }
                Phase::Running { turn_id: owned, .. } if owned == turn_id => {
                    job.phase = Self::after_terminal(job.interval, "종료");
                }
                _ => {}
            }
        }
        self.prune_removed();
    }

    pub(crate) fn remove(&mut self, id: u64) -> Result<(), String> {
        self.job_mut(id)?.removed = true;
        self.prune_removed();
        Ok(())
    }

    fn prune_removed(&mut self) {
        self.jobs.retain(|job| !job.removed || job.holds_target());
    }

    pub(crate) fn pause(&mut self, id: u64) -> Result<(), String> {
        self.job_mut(id)?.paused = true;
        Ok(())
    }

    pub(crate) fn resume(&mut self, id: u64, now: Instant) -> Result<(), String> {
        let job = self.job_mut(id)?;
        if job.removed {
            return Err("제거한 예약은 다시 시작할 수 없습니다.".into());
        }
        if matches!(
            job.phase,
            Phase::Unknown(_)
                | Phase::BlockedUnsupported(_)
                | Phase::BlockedConnection(_)
                | Phase::Pending { .. }
        ) {
            return Err("실행 상태가 미확인입니다. 자동 재전송하지 않습니다.".into());
        }
        if matches!(job.phase, Phase::Finished(_)) && job.interval.is_none() {
            return Err("종료한 일회성 예약입니다. 새로 등록하세요.".into());
        }
        job.paused = false;
        if !matches!(job.phase, Phase::Running { .. }) {
            job.phase = Phase::Ready;
        }
        job.next_due = Some(match job.interval {
            Some(interval) => now
                .checked_add(interval)
                .ok_or("예약 간격이 너무 큽니다.")?,
            None => job.next_due.unwrap_or(now).max(now),
        });
        Ok(())
    }

    pub(crate) fn block_unsupported(&mut self, target_matches: impl Fn(&T) -> bool) {
        for job in self
            .jobs
            .iter_mut()
            .filter(|job| target_matches(&job.target))
        {
            job.phase = Phase::BlockedUnsupported("turn/startIfIdle 미지원: 실행되지 않음".into());
            job.next_due = None;
        }
    }

    pub(crate) fn invalidate(&mut self, target_matches: impl Fn(&T) -> bool, reason: &str) {
        for job in self
            .jobs
            .iter_mut()
            .filter(|job| target_matches(&job.target))
        {
            job.phase = match &job.phase {
                Phase::Pending { .. } | Phase::Running { .. } | Phase::Unknown(_) => {
                    Phase::Unknown(reason.into())
                }
                Phase::Finished(_) | Phase::BlockedUnsupported(_) => continue,
                _ => Phase::BlockedConnection(reason.into()),
            };
            job.next_due = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn recurring(s: &mut Scheduler<&str>, now: Instant) -> u64 {
        s.add(
            "thread-A/epoch-1",
            "saved prompt".into(),
            Schedule::Every(Duration::from_secs(10)),
            now,
        )
        .unwrap()
    }

    #[test]
    fn missed_slots_busy_and_terminal_are_not_a_queue() {
        let now = Instant::now();
        let mut s = Scheduler::default();
        let id = recurring(&mut s, now);
        assert!(s.due(now).is_empty());
        let d = s.due(now + Duration::from_secs(97)).pop().unwrap();
        assert_eq!(
            (d.id, d.target, d.prompt.as_str()),
            (id, "thread-A/epoch-1", "saved prompt")
        );
        assert_eq!(s.next_deadline(), Some(now + Duration::from_secs(107)));
        assert!(s.submitted(id, d.attempt, Submission::Busy));
        assert_eq!(s.jobs[0].skipped, 1);
        assert!(s.due(now + Duration::from_secs(97)).is_empty());
        let d = s.due(now + Duration::from_secs(107)).pop().unwrap();
        s.submitted(id, d.attempt, Submission::Started("owned".into()));
        assert!(s.due(now + Duration::from_secs(117)).is_empty());
        s.terminal(&"thread-B/epoch-1", "owned");
        s.terminal(&"thread-A/epoch-1", "manual-turn");
        assert!(matches!(s.jobs[0].phase, Phase::Running { .. }));
        s.terminal(&"thread-A/epoch-1", "owned");
        assert!(matches!(s.jobs[0].phase, Phase::Ready));
    }

    #[test]
    fn completion_before_reply_duplicate_reply_and_remove_preserve_exact_owner() {
        let now = Instant::now();
        let mut s = Scheduler::default();
        let id = recurring(&mut s, now);
        let d = s.due(now + Duration::from_secs(10)).pop().unwrap();
        s.terminal(&d.target, "fast-turn");
        assert!(!s.submitted(id, d.attempt + 1, Submission::Started("wrong".into())));
        assert!(s.submitted(id, d.attempt, Submission::Started("fast-turn".into())));
        assert!(!s.submitted(id, d.attempt, Submission::Started("duplicate".into())));
        let d = s.due(now + Duration::from_secs(20)).pop().unwrap();
        s.remove(id).unwrap();
        assert_eq!(s.jobs.len(), 1);
        s.submitted(id, d.attempt, Submission::Started("owned".into()));
        assert!(s.due(now + Duration::from_secs(100)).is_empty());
        s.terminal(&d.target, "other");
        assert_eq!(s.jobs.len(), 1);
        s.terminal(&d.target, "owned");
        assert!(s.jobs.is_empty());
    }

    #[test]
    fn one_shot_busy_is_terminal_and_unknown_is_not_retried() {
        let now = Instant::now();
        let mut s = Scheduler::default();
        let id = s
            .add("A", "prompt".into(), Schedule::Once(now), now)
            .unwrap();
        let d = s.due(now).pop().unwrap();
        s.submitted(id, d.attempt, Submission::Busy);
        assert!(matches!(s.jobs[0].phase, Phase::Finished(_)));
        assert!(s.resume(id, now).is_err());
        assert!(s.next_deadline().is_none());
        let id = recurring(&mut s, now);
        let d = s.due(now + Duration::from_secs(10)).pop().unwrap();
        s.submitted(id, d.attempt, Submission::Unknown("transport".into()));
        assert!(s.resume(id, now).is_err());
        assert!(s.due(now + Duration::from_secs(100)).is_empty());
        s.invalidate(|_| true, "new epoch");
        assert!(s.next_deadline().is_none());
    }

    #[test]
    fn pause_resume_and_remove_never_cancel_an_owned_turn() {
        let now = Instant::now();
        let mut s = Scheduler::default();
        let id = recurring(&mut s, now);
        s.pause(id).unwrap();
        assert!(s.next_deadline().is_none());
        s.resume(id, now + Duration::from_secs(100)).unwrap();
        assert!(s.due(now + Duration::from_secs(109)).is_empty());
        let d = s.due(now + Duration::from_secs(110)).pop().unwrap();
        s.submitted(id, d.attempt, Submission::Started("owned".into()));
        s.pause(id).unwrap();
        s.resume(id, now + Duration::from_secs(111)).unwrap();
        assert!(
            matches!(&s.jobs[0].phase, Phase::Running { turn_id, cancelling: false } if turn_id == "owned")
        );
        s.remove(id).unwrap();
        assert_eq!(s.jobs.len(), 1);
    }
    #[test]
    fn explicit_non_admission_and_unsupported_do_not_become_unknown_or_retry() {
        let now = Instant::now();
        let mut s = Scheduler::default();
        let id = recurring(&mut s, now);
        let dispatch = s.due(now + Duration::from_secs(10)).pop().unwrap();
        s.submitted(
            id,
            dispatch.attempt,
            Submission::NotStarted("ServerDraining".into()),
        );
        assert!(matches!(s.jobs[0].phase, Phase::Finished(_)));
        assert!(!s.jobs[0].holds_target());
        assert!(s.next_deadline().is_none());
        s.invalidate(|_| true, "connection replaced after explicit refusal");
        assert!(
            matches!(s.jobs[0].phase, Phase::Finished(_)),
            "known refusal is not an uncertain execution"
        );
        s.remove(id).unwrap();
        assert!(s.jobs.is_empty());
        recurring(&mut s, now);
        s.add(
            "thread-B/epoch-1",
            "other prompt".into(),
            Schedule::Every(Duration::from_secs(20)),
            now,
        )
        .unwrap();
        s.block_unsupported(|target| *target == "thread-A/epoch-1");
        assert!(matches!(s.jobs[0].phase, Phase::BlockedUnsupported(_)));
        assert!(!s.jobs[0].holds_target());
        let due = s.due(now + Duration::from_secs(100));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].target, "thread-B/epoch-1");
    }

    #[test]
    fn invalidated_attempt_cannot_publish_late_reply_into_new_epoch() {
        let now = Instant::now();
        let mut s = Scheduler::default();
        let id = recurring(&mut s, now);
        let dispatch = s.due(now + Duration::from_secs(10)).pop().unwrap();
        s.invalidate(|target| *target == dispatch.target, "connection replaced");
        assert!(!s.submitted(
            id,
            dispatch.attempt,
            Submission::Started("late-old-turn".into())
        ));
        let new = s
            .add(
                "thread-A/epoch-2",
                "new prompt".into(),
                Schedule::Once(now),
                now,
            )
            .unwrap();
        let dispatch = s.due(now).pop().unwrap();
        s.submitted(
            new,
            dispatch.attempt,
            Submission::Started("new-turn".into()),
        );
        s.terminal(&"thread-A/epoch-1", "new-turn");
        assert!(
            matches!(&s.jobs[1].phase, Phase::Running { turn_id, .. } if turn_id == "new-turn")
        );
        s.terminal(&"thread-A/epoch-2", "new-turn");
        assert!(matches!(s.jobs[1].phase, Phase::Finished(_)));
    }
}
