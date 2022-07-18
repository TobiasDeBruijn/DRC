#![allow(warnings)]

use std::collections::HashMap;
use crate::args::Args;
use crate::docker::{delete_digest, get_blob, get_tag_digest, list_repositories, list_tags, Repository, Tag, TagDigest};
use anyhow::Result;
use futures::future::join_all;
use std::future::Future;
use std::pin::Pin;
use tracing::{debug, info, Level, warn};
use tracing_subscriber::fmt::writer::MakeWriterExt;

mod args;
mod docker;

#[tokio::main]
async fn main() {
    let args = Args::new();
    configure_tracing(args.debug, args.trace);

    if args.dry_run {
        warn!("Dry run is enabled. No images will be deleted!");
    }

    let start = time::Instant::now();

    process(args).await.unwrap();

    let delta = time::Instant::now() - start;
    info!("Done. Took {}", fmt_duration(delta));
}

fn fmt_duration(duration: time::Duration) -> String {
    if duration.whole_seconds() > 0 {
        format!("{}s", duration.whole_seconds())
    } else if duration.whole_milliseconds() > 0 {
        format!("{}ms", duration.whole_milliseconds())
    } else {
        format!("{}µs", duration.whole_microseconds())
    }
}

async fn process(args: Args) -> Result<()> {
    debug!("Collecting repositories");
    let repositories = list_repositories(&args.registry).await?;
    debug!("Collecting tags");
    let tags = collect_tasks(&args.registry, &repositories, list_tags).await?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    let mut map: HashMap<&Repository, Vec<Tag>> = HashMap::new();
    for tag in tags {
        // We do this option shennanigans to avoid cloning
        // https://users.rust-lang.org/t/how-to-avoid-redundant-cloning-on-hashmap-insertion/23743/4
        let mut tag_option = Some(tag);
        let repository = tag_option.as_ref().unwrap().1;

        map.entry(repository)
            .and_modify(|x| x.push(tag_option.take().unwrap()))
            .or_insert_with(|| vec![tag_option.unwrap()]);
    }

    debug!("Filtering repositories to keep");
    let to_process_tags = map.into_iter()
        .filter(|(repository, tags)| {
            // We count how many 'free' tags there are
            // If there are no free tags, we don't touch the repository
            // A free tag is defined as:
            // - a tag that is not named 'latest'
            // - a tag of which the name does not start with 'v'
            // The reason for this distinction is to avoid breaking deployments
            // which might depend on the latest tag or a specific version

            // This is only ever 0 or 1
            // We could represent this as a boolean but
            // we're using it for addition later, so converting
            // makes no sense
            let latest_tag = tags.iter()
                .filter(|x| x.0.eq("latest"))
                .count();

            let version_tags = tags.iter()
                .filter(|x| x.0.starts_with("v"))
                .count();

            let required_tags = 1 + latest_tag + version_tags;
            if tags.len() > required_tags {
                debug!("Continueing with Repository {} because it has free tags (it has {} tags, {version_tags} version tags, and {latest_tag} latest tags)", repository.0, tags.len());
                true
            } else {
                debug!("Not continueing with Repository {}, because it has no free tags (it has {} tags, {version_tags} version tags, and {latest_tag} latest tags)", repository.0, tags.len());
                false
            }
        })
        .map(|(_, tags)| tags)
        .flatten()
        .collect::<Vec<_>>();

    debug!("Collecting digests");
    let digests = collect_tasks(&args.registry, &to_process_tags, get_tag_digest).await?;
    debug!("Collecting blobs");
    let blobs = collect_tasks(&args.registry, &digests, get_blob).await?;

    debug!("Filtering tags");
    let delete_before = (time::OffsetDateTime::now_utc() - time::Duration::seconds(args.retention as i64)).unix_timestamp();
    let to_delete = blobs.iter()
        .filter(|x| x.date < delete_before)
        .collect::<Vec<_>>();

    if args.dry_run {
        info!("Dry run is enabled. If it were not, the following images would be deleted:");
        for blob in to_delete {
            info!("- {}/{} (Age: {})", blob.tag_digest.1.1.0, blob.tag_digest.1.0, fmt_age(blob.date));
        }
    } else {
        for blob in to_delete {
            info!("Deleting image {}/{}", blob.tag_digest.1.1.0, blob.tag_digest.1.0);
            delete_digest(&args.registry, blob.tag_digest).await?;
        }
    }

    Ok(())
}

fn fmt_age(epoch: i64) -> String {
    let age = time::OffsetDateTime::now_utc().unix_timestamp() - epoch;

    if age > 86400 {
        format!("{} Days", age / 86400)
    } else if age > 3600 {
        format!("{} Hours", age / 3600)
    } else if age > 60 {
        format!("{} Minutes", age / 60)
    } else {
        format!("{age} Seconds")
    }
}

async fn collect_tasks<'a, 'b, I, O, F>(registry: &'b str, input: &'a [I], applied: fn(&'b str, &'a I) -> F) -> Result<Vec<O>>
where
    I: 'a,
    O: 'a,
    F: Future<Output = Result<O>>,
{
    let tasks = input
        .iter()
        .map(|x| applied(registry, x))
        .collect::<Vec<_>>();
    let collected = join_all(tasks)
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>();
    collected
}

fn configure_tracing(debug: bool, trace: bool) {
    let level = if trace {
        Level::TRACE
    } else if debug {
        Level::DEBUG
    } else {
        Level::INFO
    };

    let subscriber = tracing_subscriber::fmt()
        .compact()
        .with_max_level(level)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("Setting tracing subscriber");
}
