use lazy_static::lazy_static;
use reqwest::{Client, Response};
use anyhow::Result;
use serde::{Serialize, Deserialize};
use time::OffsetDateTime;
use tracing::{error_span, info, instrument};
use tracing_subscriber::Registry;

lazy_static! {
    static ref CLIENT: Client = Client::new();
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Repository(pub String);

#[instrument]
pub async fn list_repositories(registry: &str) -> Result<Vec<Repository>> {
    #[derive(Debug, Deserialize)]
    struct Response {
        repositories: Option<Vec<String>>
    }

    let response: Response = CLIENT.get(format!("{registry}/v2/_catalog"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let result = response.repositories
        .unwrap_or(Vec::new())
        .into_iter().map(|x| Repository(x)).collect::<Vec<_>>();
    Ok(result)
}

#[derive(Debug)]
pub struct Tag<'a>(pub String, pub &'a Repository);

#[instrument]
pub async fn list_tags<'a, 'b>(registry: &'b str, repository: &'a Repository) -> Result<Vec<Tag<'a>>> {
    #[derive(Debug, Deserialize)]
    struct Response {
        tags: Option<Vec<String>>
    }

    let response: Response = CLIENT.get(format!("{registry}/v2/{}/tags/list", repository.0))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let result = response.tags
        .unwrap_or(Vec::default())
        .into_iter().map(|x| Tag(x, repository)).collect::<Vec<_>>();
    Ok(result)
}

#[derive(Debug)]
pub struct TagDigest<'a>(String, pub &'a Tag<'a>);

#[instrument]
pub async fn get_tag_digest<'a, 'b>(registry: &'b str, tag: &'a Tag<'a>) -> Result<TagDigest<'a>> {
    #[derive(Debug, Deserialize)]
    struct Response {
        config: Config
    }

    #[derive(Debug, Deserialize)]
    struct Config {
        digest: String,
    }

    let response: Response = CLIENT.get(format!("{registry}/v2/{}/manifests/{}", tag.1.0, tag.0))
        .header("Accept", "application/vnd.docker.distribution.manifest.v2+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(TagDigest(response.config.digest, tag))
}

#[derive(Debug)]
pub struct Blob<'a> {
    pub tag_digest: &'a TagDigest<'a>,
    pub date: i64,
}

#[instrument]
pub async fn get_blob<'a, 'b>(registry: &'b str, digest: &'a TagDigest<'a>) -> Result<Blob<'a>> {
    #[derive(Debug, Deserialize)]
    struct Response {
        created: String
    }

    let response: Response = CLIENT.get(format!("{registry}/v2/{}/blobs/{}", digest.1.1.0, digest.0))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let parsed_time = OffsetDateTime::parse(&response.created, &time::format_description::well_known::Iso8601::PARSING)?;
    Ok(Blob {
        tag_digest: digest,
        date: parsed_time.unix_timestamp()
    })
}

#[instrument]
pub async fn delete_digest(registry: &str, digest: &TagDigest<'_>) -> Result<()> {
    CLIENT.delete(format!("{registry}/v2/{}/manifests/{}", digest.1.1.0, digest.0))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}