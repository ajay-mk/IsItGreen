//! Images in pull request and issue descriptions.
//!
//! The webview's content security policy only allows images from the app
//! itself and `data:` URLs, so a description cannot make the app contact an
//! arbitrary server (a tracking pixel, say). Instead the frontend asks for an
//! image here, and it comes back as a `data:` URL.
//!
//! Only GitHub's own image hosts are fetched, redirects are followed only
//! between them, and no token is ever sent: images in private repositories
//! are reached through the short-lived signed URLs that GitHub puts in a
//! description's rendered HTML.

use base64::Engine as _;
use reqwest::{header, StatusCode, Url};

use crate::error::{Error, Result};
use crate::http::{client, read_body};

/// Hosts that serve images GitHub stores or proxies.
const IMAGE_HOSTS: &[&str] = &[
    // Attachments in private repositories, through signed URLs.
    "private-user-images.githubusercontent.com",
    // Older attachments.
    "user-images.githubusercontent.com",
    // GitHub's image proxy, which fetches images from other sites for it.
    "camo.githubusercontent.com",
    // Public repositories only: private raw files need a token, which is
    // never sent, so they stay links.
    "raw.githubusercontent.com",
    "objects.githubusercontent.com",
    "avatars.githubusercontent.com",
    // Where github.com/user-attachments links to public attachments redirect.
    "github-production-user-asset-6210df.s3.amazonaws.com",
];

/// Image types an `<img>` can show. SVG is included: in an `<img>` it cannot
/// run scripts or load anything.
const IMAGE_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/avif",
    "image/bmp",
    "image/svg+xml",
];

/// Redirects followed for one image, all within the allowed hosts.
const MAX_REDIRECTS: usize = 3;

/// Whether `url` is somewhere GitHub serves description images from: one of
/// its image hosts, or an attachment link on github.com itself.
pub fn is_image_url(url: &Url) -> bool {
    if url.scheme() != "https"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    match url.host_str() {
        Some("github.com") => {
            let segments: Vec<&str> = url
                .path_segments()
                .map(Iterator::collect)
                .unwrap_or_default();
            matches!(segments.as_slice(), ["user-attachments", "assets", id] if !id.is_empty())
                // The older form, under the repository.
                || matches!(segments.as_slice(), [owner, repo, "assets", ..] if !owner.is_empty() && !repo.is_empty())
        }
        Some(host) => IMAGE_HOSTS.contains(&host),
        None => false,
    }
}

/// The image type of a response, if it is one an `<img>` may show.
fn image_type(content_type: Option<&str>) -> Option<&'static str> {
    let mime = content_type?.split(';').next()?.trim().to_ascii_lowercase();
    IMAGE_TYPES.iter().copied().find(|t| *t == mime)
}

/// Where a redirect points, if it is allowed.
fn redirect_target(from: &Url, location: Option<&str>) -> Result<Url> {
    let target = from
        .join(location.ok_or(Error::ForbiddenUrl)?)
        .map_err(|_| Error::ForbiddenUrl)?;
    if is_image_url(&target) {
        Ok(target)
    } else {
        Err(Error::ForbiddenUrl)
    }
}

pub struct ImageClient {
    http: reqwest::Client,
}

impl ImageClient {
    pub fn new() -> Result<Self> {
        // The shared client: HTTPS only, no automatic redirects, timeouts.
        Ok(Self { http: client()? })
    }

    /// Fetches one image and returns it as a `data:` URL.
    pub async fn fetch(&self, url: Url) -> Result<String> {
        if !is_image_url(&url) {
            return Err(Error::ForbiddenUrl);
        }
        let mut url = url;
        for _ in 0..=MAX_REDIRECTS {
            let response = self
                .http
                .get(url.clone())
                .header(header::ACCEPT, "image/*")
                .send()
                .await?;
            let status = response.status();
            if status.is_redirection() {
                let location = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|v| v.to_str().ok());
                url = redirect_target(&url, location)?;
                continue;
            }
            if status != StatusCode::OK {
                return Err(Error::Http(status.as_u16()));
            }
            let mime = image_type(
                response
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok()),
            )
            .ok_or(Error::ForbiddenUrl)?;
            let body = read_body(response).await?;
            let data = base64::engine::general_purpose::STANDARD.encode(body);
            return Ok(format!("data:{mime};base64,{data}"));
        }
        Err(Error::ForbiddenUrl)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn image_hosts() {
        for ok in [
            "https://github.com/user-attachments/assets/0a1b2c3d-1111-2222-3333-444455556666",
            "https://github.com/acme/api/assets/123/abc",
            "https://private-user-images.githubusercontent.com/1/2-uuid.png?jwt=x",
            "https://camo.githubusercontent.com/abc/def",
            "https://raw.githubusercontent.com/a/b/main/x.png",
            "https://github-production-user-asset-6210df.s3.amazonaws.com/1/x.png?X-Amz=1",
        ] {
            assert!(is_image_url(&url(ok)), "{ok}");
        }
        for bad in [
            // Other github.com pages are not images.
            "https://github.com/acme/api",
            "https://github.com/user-attachments/assets/",
            "https://github.com/login",
            "https://gist.githubusercontent.com/a/b/raw/x",
            "https://example.com/pixel.gif",
            "https://raw.githubusercontent.com.evil.com/x.png",
            "http://raw.githubusercontent.com/a/b/x.png",
            "https://raw.githubusercontent.com:8443/a/b/x.png",
            "https://x@raw.githubusercontent.com/a/b/x.png",
            "https://s3.amazonaws.com/other-bucket/x.png",
            "data:image/png;base64,AAAA",
            "file:///etc/passwd",
        ] {
            assert!(!is_image_url(&url(bad)), "{bad}");
        }
    }

    #[test]
    fn image_types() {
        assert_eq!(image_type(Some("image/png")), Some("image/png"));
        assert_eq!(
            image_type(Some("IMAGE/JPEG; charset=binary")),
            Some("image/jpeg")
        );
        assert_eq!(image_type(Some("image/svg+xml")), Some("image/svg+xml"));
        assert_eq!(image_type(Some("text/html")), None);
        assert_eq!(image_type(Some("application/octet-stream")), None);
        assert_eq!(image_type(Some("image/x-icon")), None);
        assert_eq!(image_type(None), None);
    }

    #[test]
    fn redirects_stay_on_image_hosts() {
        let from = url("https://github.com/user-attachments/assets/0a1b");
        assert_eq!(
            redirect_target(
                &from,
                Some("https://private-user-images.githubusercontent.com/1/x.png?jwt=t")
            )
            .unwrap(),
            url("https://private-user-images.githubusercontent.com/1/x.png?jwt=t"),
        );
        assert!(redirect_target(&from, Some("https://example.com/x.png")).is_err());
        // A relative redirect to another github.com page that is not an image.
        assert!(redirect_target(&from, Some("/login")).is_err());
        assert!(redirect_target(&from, None).is_err());
    }

    /// Fetches a real image over the network; run with `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs network access"]
    fn fetches_a_real_image() {
        let client = ImageClient::new().unwrap();
        let data = tauri::async_runtime::block_on(client.fetch(url(
            "https://raw.githubusercontent.com/ajay-mk/IsItGreen/screenshots/pr-56/dashboard-light.png",
        )))
        .unwrap();
        assert!(data.starts_with("data:image/png;base64,iVBORw0KGgo"));
        // Not an image host: refused before any request.
        let refused =
            tauri::async_runtime::block_on(client.fetch(url("https://example.com/x.png")));
        assert!(matches!(refused, Err(Error::ForbiddenUrl)));
    }
}
