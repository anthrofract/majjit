use std::{
    io::ErrorKind,
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, channel, sync_channel},
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{Result, anyhow, bail};
use tokio::sync::oneshot;
use watchman_client::{SubscriptionData, prelude::*};

pub struct WatchmanMonitor {
    changes: Receiver<()>,
    errors: Receiver<anyhow::Error>,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl WatchmanMonitor {
    pub fn start(repository: String) -> Result<Option<Self>> {
        if !watchman_available()? {
            return Ok(None);
        }

        let (change_tx, changes) = sync_channel(1);
        let (error_tx, errors) = channel();
        let (shutdown, shutdown_rx) = oneshot::channel();
        let thread = thread::Builder::new()
            .name("majjit-watchman".to_string())
            .spawn(move || {
                if let Err(err) = watch(repository.into(), change_tx, shutdown_rx) {
                    let _ = error_tx.send(err);
                }
            })?;

        Ok(Some(Self {
            changes,
            errors,
            shutdown: Some(shutdown),
            thread: Some(thread),
        }))
    }

    pub fn take_change(&self) -> Result<bool> {
        match self.errors.try_recv() {
            Ok(err) => return Err(err),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                return Err(anyhow!("Watchman monitor stopped unexpectedly"));
            }
        }

        match self.changes.try_recv() {
            Ok(()) => Ok(true),
            Err(TryRecvError::Empty) => Ok(false),
            Err(TryRecvError::Disconnected) => {
                Err(anyhow!("Watchman monitor stopped unexpectedly"))
            }
        }
    }
}

impl Drop for WatchmanMonitor {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn watchman_available() -> Result<bool> {
    match Command::new("watchman")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) if status.success() => Ok(true),
        Ok(status) => Err(anyhow!("`watchman --version` failed with {status}")),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn watch(
    repository: PathBuf,
    changes: SyncSender<()>,
    shutdown: oneshot::Receiver<()>,
) -> Result<()> {
    let jj_dir = repository.join(".jj");
    let repo_path = jj_dir.join("repo");
    let repo_path = if repo_path.is_file() {
        // Additional workspaces point at the shared repository rather than
        // keeping their own operation store.
        jj_dir.join(std::fs::read_to_string(repo_path)?)
    } else {
        repo_path
    };
    let operation_heads = repo_path.join("op_heads").join("heads");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async move {
        let subscriptions = async {
            tokio::try_join!(
                subscribe(repository, changes.clone()),
                poll_operation_heads(operation_heads, changes),
            )?;
            Ok(())
        };
        tokio::select! {
            _ = shutdown => Ok(()),
            result = subscriptions => result,
        }
    })
}

async fn subscribe(repository: PathBuf, changes: SyncSender<()>) -> Result<()> {
    let client = Connector::new().connect().await?;
    let root = client
        .resolve_root(CanonicalPath::canonicalize(repository)?)
        .await?;
    let expression = Expr::Not(Box::new(Expr::Any(vec![
        Expr::DirName(DirNameTerm {
            path: ".git".into(),
            depth: None,
        }),
        Expr::DirName(DirNameTerm {
            path: ".jj".into(),
            depth: None,
        }),
    ])));
    let clock = client.clock(&root, SyncTimeout::Default).await?;
    let (mut subscription, _) = client
        .subscribe::<NameOnly>(
            &root,
            SubscribeRequest {
                since: Some(Clock::Spec(clock)),
                expression: Some(expression),
                ..Default::default()
            },
        )
        .await?;

    if !send_change(&changes) {
        return Ok(());
    }

    loop {
        match subscription.next().await? {
            SubscriptionData::FilesChanged(result)
                if result.is_fresh_instance
                    || result.files.as_ref().is_some_and(|files| !files.is_empty()) =>
            {
                if !send_change(&changes) {
                    return Ok(());
                }
            }
            SubscriptionData::Canceled => bail!("Watchman subscription was canceled"),
            SubscriptionData::FilesChanged(_)
            | SubscriptionData::StateEnter { .. }
            | SubscriptionData::StateLeave { .. } => {}
        }
    }
}

async fn poll_operation_heads(operation_heads: PathBuf, changes: SyncSender<()>) -> Result<()> {
    // Watchman's project watches may skip .jj metadata. Poll only the small
    // shared head set, without running jj unless that set actually changes.
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut previous = Vec::new();
    let mut current = Vec::new();
    loop {
        interval.tick().await;
        current.clear();
        for entry in std::fs::read_dir(&operation_heads)? {
            current.push(entry?.file_name());
        }
        current.sort_unstable();
        if current != previous && !send_change(&changes) {
            return Ok(());
        }
        std::mem::swap(&mut previous, &mut current);
    }
}

fn send_change(changes: &SyncSender<()>) -> bool {
    match changes.try_send(()) {
        Ok(()) | Err(TrySendError::Full(())) => true,
        Err(TrySendError::Disconnected(())) => false,
    }
}
