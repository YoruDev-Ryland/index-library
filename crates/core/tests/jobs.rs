use lo_core::jobs::{JobEvent, JobKind, JobManager, JobState};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn manager() -> (JobManager, Arc<Mutex<Vec<JobEvent>>>) {
    let ev = Arc::new(Mutex::new(Vec::new()));
    let e2 = ev.clone();
    (JobManager::new(Arc::new(move |e: JobEvent| e2.lock().unwrap().push(e))), ev)
}

#[test]
fn same_kind_runs_sequentially() {
    let (m, _) = manager();
    let spans = Arc::new(Mutex::new(Vec::<(Instant, Instant)>::new()));
    for _ in 0..2 {
        let s = spans.clone();
        m.submit(JobKind::Hash, "h", Box::new(move |_ctx| { let a = Instant::now(); std::thread::sleep(Duration::from_millis(80)); s.lock().unwrap().push((a, Instant::now())); Ok(()) }));
    }
    assert!(m.wait_idle(Duration::from_secs(5)));
    let s = spans.lock().unwrap();
    assert_eq!(s.len(), 2);
    assert!(s[1].0 >= s[0].1 || s[0].0 >= s[1].1, "overlapped");
}

#[test]
fn different_kinds_run_concurrently() {
    let (m, _) = manager();
    let start = Instant::now();
    m.submit(JobKind::Hash, "h", Box::new(|_| { std::thread::sleep(Duration::from_millis(150)); Ok(()) }));
    m.submit(JobKind::Scan, "s", Box::new(|_| { std::thread::sleep(Duration::from_millis(150)); Ok(()) }));
    assert!(m.wait_idle(Duration::from_secs(5)));
    assert!(start.elapsed() < Duration::from_millis(280), "{:?}", start.elapsed());
}

#[test]
fn cancel_finishes_with_cancelled() {
    let (m, ev) = manager();
    let id = m.submit(JobKind::Action, "a", Box::new(|ctx| {
        while !ctx.cancelled() { std::thread::sleep(Duration::from_millis(5)); }
        anyhow::bail!("cancelled")
    }));
    std::thread::sleep(Duration::from_millis(30));
    m.cancel(id);
    assert!(m.wait_idle(Duration::from_secs(5)));
    let info = m.list().into_iter().find(|j| j.id == id).unwrap();
    assert_eq!(info.state, JobState::Cancelled);
    let ev = ev.lock().unwrap();
    assert!(matches!(ev.last(), Some(JobEvent::Finished { ok: false, .. })));
}

#[test]
fn events_in_order_with_progress_and_log() {
    let (m, ev) = manager();
    let id = m.submit(JobKind::Metadata, "m", Box::new(|ctx| { ctx.progress(1, 2, "half"); ctx.log("hello"); ctx.progress(2, 2, "done"); Ok(()) }));
    assert!(m.wait_idle(Duration::from_secs(5)));
    let ev = ev.lock().unwrap();
    let kinds: Vec<&str> = ev.iter().filter(|e| e.job_id() == id).map(|e| match e { JobEvent::Queued { .. } => "q", JobEvent::Started { .. } => "s", JobEvent::Progress { .. } => "p", JobEvent::Log { .. } => "l", JobEvent::Finished { .. } => "f" }).collect();
    assert_eq!(kinds.first(), Some(&"q"));
    assert_eq!(kinds[1], "s");
    assert_eq!(kinds.last(), Some(&"f"));
    assert!(kinds.contains(&"l"));
    let info = m.list().into_iter().find(|j| j.id == id).unwrap();
    assert_eq!((info.done, info.total, info.state), (2, 2, JobState::Done));
    assert_eq!(info.log, vec!["hello".to_string()]);
}

#[test]
fn panicking_job_is_reported_not_fatal() {
    let (m, _) = manager();
    let id = m.submit(JobKind::Scan, "boom", Box::new(|_| panic!("kaboom")));
    assert!(m.wait_idle(Duration::from_secs(5)));
    let info = m.list().into_iter().find(|j| j.id == id).unwrap();
    assert_eq!(info.state, JobState::Failed);
    let id2 = m.submit(JobKind::Scan, "after", Box::new(|_| Ok(())));
    assert!(m.wait_idle(Duration::from_secs(5)));
    assert_eq!(m.list().into_iter().find(|j| j.id == id2).unwrap().state, JobState::Done);
}
