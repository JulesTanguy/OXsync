use std::path::Path;
use std::process::abort;

use clap::Parser;
use notify::{Config, Event, RecommendedWatcher, Watcher};
use tokio::fs::canonicalize;
use tokio::sync::mpsc::unbounded_channel;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::{Args, LOG_TRACE};
use crate::utils::Utils;

pub(crate) struct Start;

impl Start {
    pub async fn parse_args() {
        let mut args = Args::parse();

        LOG_TRACE.set(args.trace).unwrap();

        if !Path::new(&args.source_dir).exists() {
            eprintln!(
                "source dir : '{}' does not exists",
                Utils::fmt_path(&args.source_dir)
            );
            abort()
        }

        if !Path::new(&args.target_dir).exists() {
            eprintln!(
                "target dir : '{}' does not exists",
                Utils::fmt_path(&args.target_dir)
            );
            abort()
        }

        args.source_dir = canonicalize(Path::new(&args.source_dir))
            .await
            .unwrap_or_else(|_| {
                eprintln!(
                    "impossible to convert source dir '{}' to a valid path",
                    Utils::fmt_path(&args.source_dir)
                );
                abort()
            });

        args.target_dir = canonicalize(Path::new(&args.target_dir))
            .await
            .unwrap_or_else(|_| {
                eprintln!(
                    "impossible to convert target dir '{}' to a valid path",
                    Utils::fmt_path(&args.target_dir)
                );
                abort()
            });

        let mut excluded_paths = Vec::new();

        for path in &args.exclude {
            let full_path = if !path.starts_with(&args.source_dir) {
                args.source_dir.as_path().join(path)
            } else {
                path.to_path_buf()
            };

            excluded_paths.push(Utils::path_to_verbatim(&full_path));
        }

        if args.ide_mode {
            excluded_paths.push(Utils::path_to_verbatim(&args.source_dir.join(".idea")));
            excluded_paths.push(Utils::path_to_verbatim(&args.source_dir.join(".git")));
            args.exclude_temporary_editor_files = true;
        }

        excluded_paths.shrink_to_fit();
        Utils::set_excluded_paths(excluded_paths);

        Utils::set_args(args);
    }

    pub fn fs_watcher() -> notify::Result<(
        RecommendedWatcher,
        UnboundedReceiverStream<notify::Result<Event>>,
    )> {
        let (tx, rx) = unbounded_channel();

        // Automatically select the best implementation for your platform.
        // You can also access each implementation directly e.g. INotifyWatcher.
        let watcher = RecommendedWatcher::new(move |res| tx.send(res).unwrap(), Config::default())?;

        Ok((watcher, UnboundedReceiverStream::new(rx)))
    }
}
