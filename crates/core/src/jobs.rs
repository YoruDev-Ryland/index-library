//! Background job queue: one lane per kind so a long hash never blocks a scan or an action.
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobKind { Scan, Hash, Action, Metadata, Other }

impl JobKind {
    fn lanes(self) -> usize { match self { JobKind::Action => 2, _ => 1 } }
    const ALL: [JobKind; 5] = [JobKind::Scan, JobKind::Hash, JobKind::Action, JobKind::Metadata, JobKind::Other];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState { Queued, Running, Done, Failed, Cancelled }

pub type JobId = u64;

#[derive(Debug, Clone, Serialize)]
pub struct JobInfo {
    pub id: JobId,
    pub kind: JobKind,
    pub label: String,
    pub state: JobState,
    pub done: u64,
    pub total: u64,
    pub message: String,
    pub error: Option<String>,
    pub log: Vec<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum JobEvent {
    Queued { id: JobId, kind: JobKind, label: String },
    Started { id: JobId },
    Progress { id: JobId, done: u64, total: u64, message: String },
    Log { id: JobId, line: String },
    Finished { id: JobId, ok: bool, error: Option<String> },
}

impl JobEvent {
    pub fn job_id(&self) -> JobId {
        match self { JobEvent::Queued { id, .. } | JobEvent::Started { id } | JobEvent::Progress { id, .. } | JobEvent::Log { id, .. } | JobEvent::Finished { id, .. } => *id }
    }
}

pub type Sink = Arc<dyn Fn(JobEvent) + Send + Sync>;
pub type JobFn = Box<dyn FnOnce(&JobCtx) -> anyhow::Result<()> + Send>;

const LOG_KEEP: usize = 500;

struct Shared {
    sink: Sink,
    jobs: Mutex<HashMap<JobId, JobInfo>>,
    cancels: Mutex<HashMap<JobId, Arc<AtomicBool>>>,
    active: Mutex<usize>,
    idle: Condvar,
}

pub struct JobCtx {
    pub id: JobId,
    cancel: Arc<AtomicBool>,
    shared: Arc<Shared>,
    last_progress: Mutex<Instant>,
}

impl JobCtx {
    pub fn cancelled(&self) -> bool { self.cancel.load(Ordering::Relaxed) }
    pub fn cancel_flag(&self) -> &AtomicBool { &self.cancel }
    pub fn progress(&self, done: u64, total: u64, msg: &str) {
        if let Some(j) = self.shared.jobs.lock().unwrap().get_mut(&self.id) { j.done = done; j.total = total; j.message = msg.to_string(); }
        // at most ~10 events/s, but always the final one
        let mut last = self.last_progress.lock().unwrap();
        if done >= total || last.elapsed() >= Duration::from_millis(100) {
            *last = Instant::now();
            (self.shared.sink)(JobEvent::Progress { id: self.id, done, total, message: msg.to_string() });
        }
    }
    pub fn log(&self, line: &str) {
        if let Some(j) = self.shared.jobs.lock().unwrap().get_mut(&self.id) {
            j.log.push(line.to_string());
            if j.log.len() > LOG_KEEP { let n = j.log.len() - LOG_KEEP; j.log.drain(..n); }
        }
        (self.shared.sink)(JobEvent::Log { id: self.id, line: line.to_string() });
    }
}

#[derive(Clone)]
pub struct JobManager {
    shared: Arc<Shared>,
    lanes: Arc<HashMap<JobKind, Sender<(JobId, JobFn)>>>,
    next: Arc<AtomicU64>,
}

fn now() -> i64 { chrono::Utc::now().timestamp() }

impl JobManager {
    pub fn new(sink: Sink) -> JobManager {
        let shared = Arc::new(Shared { sink, jobs: Mutex::new(HashMap::new()), cancels: Mutex::new(HashMap::new()), active: Mutex::new(0), idle: Condvar::new() });
        let mut lanes = HashMap::new();
        for kind in JobKind::ALL {
            let (tx, rx) = channel::<(JobId, JobFn)>();
            let rx = Arc::new(Mutex::new(rx));
            for _ in 0..kind.lanes() {
                let (rx, shared) = (rx.clone(), shared.clone());
                std::thread::Builder::new().name(format!("job-{kind:?}")).spawn(move || worker(rx, shared)).expect("spawn job worker");
            }
            lanes.insert(kind, tx);
        }
        JobManager { shared, lanes: Arc::new(lanes), next: Arc::new(AtomicU64::new(1)) }
    }

    pub fn submit(&self, kind: JobKind, label: &str, run: JobFn) -> JobId {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.shared.jobs.lock().unwrap().insert(id, JobInfo { id, kind, label: label.into(), state: JobState::Queued, done: 0, total: 0, message: String::new(), error: None, log: vec![], started_at: None, finished_at: None });
        self.shared.cancels.lock().unwrap().insert(id, Arc::new(AtomicBool::new(false)));
        *self.shared.active.lock().unwrap() += 1;
        (self.shared.sink)(JobEvent::Queued { id, kind, label: label.into() });
        self.lanes[&kind].send((id, run)).expect("job lane closed");
        id
    }

    pub fn cancel(&self, id: JobId) {
        if let Some(c) = self.shared.cancels.lock().unwrap().get(&id) { c.store(true, Ordering::Relaxed); }
    }

    pub fn list(&self) -> Vec<JobInfo> {
        let mut v: Vec<JobInfo> = self.shared.jobs.lock().unwrap().values().cloned().collect();
        v.sort_by_key(|j| std::cmp::Reverse(j.id));
        v
    }

    pub fn get(&self, id: JobId) -> Option<JobInfo> { self.shared.jobs.lock().unwrap().get(&id).cloned() }

    /// Drop finished jobs from the list.
    pub fn clear_finished(&self) {
        self.shared.jobs.lock().unwrap().retain(|_, j| matches!(j.state, JobState::Queued | JobState::Running));
    }

    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let g = self.shared.active.lock().unwrap();
        let (g, _) = self.shared.idle.wait_timeout_while(g, timeout, |a| *a > 0).unwrap();
        *g == 0
    }
}

fn worker(rx: Arc<Mutex<Receiver<(JobId, JobFn)>>>, shared: Arc<Shared>) {
    loop {
        let msg = { rx.lock().unwrap().recv() };
        let Ok((id, run)) = msg else { return };
        let cancel = shared.cancels.lock().unwrap().get(&id).cloned().unwrap_or_default();
        let result: Result<(), String> = if cancel.load(Ordering::Relaxed) {
            Err("cancelled".into())
        } else {
            if let Some(j) = shared.jobs.lock().unwrap().get_mut(&id) { j.state = JobState::Running; j.started_at = Some(now()); }
            (shared.sink)(JobEvent::Started { id });
            let ctx = JobCtx { id, cancel: cancel.clone(), shared: shared.clone(), last_progress: Mutex::new(Instant::now() - Duration::from_secs(1)) };
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&ctx))) {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(format!("{e:#}")),
                Err(p) => Err(p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).map(|s| format!("crashed: {s}")).unwrap_or_else(|| "crashed".into())),
            }
        };
        let cancelled = cancel.load(Ordering::Relaxed);
        if let Some(j) = shared.jobs.lock().unwrap().get_mut(&id) {
            j.finished_at = Some(now());
            j.state = match (&result, cancelled) { (Ok(()), _) => JobState::Done, (Err(_), true) => JobState::Cancelled, (Err(_), false) => JobState::Failed };
            j.error = result.as_ref().err().cloned();
        }
        shared.cancels.lock().unwrap().remove(&id);
        (shared.sink)(JobEvent::Finished { id, ok: result.is_ok(), error: result.err() });
        let mut a = shared.active.lock().unwrap();
        *a -= 1;
        if *a == 0 { shared.idle.notify_all(); }
    }
}

