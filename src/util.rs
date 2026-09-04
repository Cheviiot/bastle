// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::HashSet, io::Cursor, sync::OnceLock, time::Duration};

use anyhow::{anyhow, bail, Context, Result};
use futures::{io::AsyncReadExt, stream, StreamExt};
use gettextrs::gettext;
use gtk::{gdk, gio, prelude::*};
use image::{imageops, DynamicImage, ImageBuffer, ImageFormat, Rgba};
use isahc::{config, prelude::*, HttpClient, Response};
use scraper::{Html, Selector};
use url::Url;

const HTML_LIMIT: usize = 2 * 1024 * 1024;
const IMAGE_LIMIT: usize = 10 * 1024 * 1024;
const ICON_SIZE: u32 = 256;
const ICON_CONTENT_SIZE: u32 = 224;
const MAX_ICON_REQUESTS: usize = 4;

#[derive(Debug, Default)]
pub struct WebsiteMeta {
    pub icon: Option<Vec<u8>>,
    pub title: Option<String>,
}

#[derive(Debug)]
struct IconCandidate {
    bytes: Vec<u8>,
    source_area: u64,
}

fn http_client() -> Result<&'static HttpClient> {
    static HTTP: OnceLock<Result<HttpClient, String>> = OnceLock::new();
    HTTP.get_or_init(|| {
        HttpClient::builder()
            .redirect_policy(config::RedirectPolicy::Limit(5))
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|error| error.to_string())
    })
    .as_ref()
    .map_err(|error| anyhow!(error.clone()))
}

fn icon_selector() -> &'static Selector {
    static SELECTOR: OnceLock<Selector> = OnceLock::new();
    SELECTOR.get_or_init(|| {
        Selector::parse(
            "link[rel='icon'], link[rel='shortcut icon'], link[rel^='apple-touch-icon']",
        )
        .expect("the built-in icon selector is valid")
    })
}

fn title_selector() -> &'static Selector {
    static SELECTOR: OnceLock<Selector> = OnceLock::new();
    SELECTOR.get_or_init(|| Selector::parse("title").expect("the built-in title selector is valid"))
}

async fn read_bounded(response: &mut Response<isahc::AsyncBody>, limit: usize) -> Result<Vec<u8>> {
    if response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > limit)
    {
        bail!("response exceeds the {limit}-byte limit");
    }
    let mut output = Vec::with_capacity(limit.min(64 * 1024));
    response
        .body_mut()
        .take((limit + 1) as u64)
        .read_to_end(&mut output)
        .await?;
    if output.len() > limit {
        bail!("response exceeds the {limit}-byte limit");
    }
    Ok(output)
}

pub async fn load_texture(buffer: Vec<u8>) -> Result<gdk::Texture> {
    let mut loader = glycin::Loader::new_vec(buffer);
    loader.sandbox_selector(glycin::SandboxSelector::Auto);
    let image = loader
        .load()
        .await
        .map_err(|error| anyhow!(error.to_string()))?;
    let mime = image.mime_type();
    let frame = if matches!(mime.as_str(), "image/svg+xml" | "image/svg+xml-compressed") {
        image
            .specific_frame(glycin::FrameRequest::new().scale(ICON_SIZE, ICON_SIZE))
            .await
    } else {
        image.next_frame().await
    }
    .map_err(|error| anyhow!(error.to_string()))?;
    Ok(frame.texture())
}

pub async fn normalize_icon(buffer: Vec<u8>) -> Result<Vec<u8>> {
    if buffer.len() > IMAGE_LIMIT {
        bail!("image exceeds the {IMAGE_LIMIT}-byte limit");
    }
    let texture = load_texture(buffer).await?;
    normalize_png_bytes(texture.save_to_png_bytes().as_ref())
}

fn normalize_png_bytes(bytes: &[u8]) -> Result<Vec<u8>> {
    let source = image::load_from_memory(bytes).context("failed to decode rendered icon")?;
    let resized = source
        .thumbnail(ICON_CONTENT_SIZE, ICON_CONTENT_SIZE)
        .to_rgba8();
    let mut canvas = ImageBuffer::from_pixel(ICON_SIZE, ICON_SIZE, Rgba([0, 0, 0, 0]));
    let x = (ICON_SIZE - resized.width()) / 2;
    let y = (ICON_SIZE - resized.height()) / 2;
    imageops::overlay(&mut canvas, &resized, i64::from(x), i64::from(y));
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(canvas)
        .write_to(&mut output, ImageFormat::Png)
        .context("failed to encode normalized icon")?;
    Ok(output.into_inner())
}

pub async fn default_icon() -> Result<Vec<u8>> {
    normalize_icon(
        include_bytes!("../data/icons/hicolor/scalable/apps/io.github.cheviiot.bastle.svg")
            .to_vec(),
    )
    .await
}

async fn fetch_icon(url: Url) -> Result<IconCandidate> {
    let mut response = http_client()?.get_async(url.to_string()).await?;
    if !response.status().is_success() {
        bail!("{url}: HTTP {}", response.status());
    }
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .map(str::to_ascii_lowercase);
    if content_type
        .as_deref()
        .is_some_and(|value| !value.starts_with("image/"))
    {
        bail!("{url}: response is not an image");
    }
    let bytes = read_bounded(&mut response, IMAGE_LIMIT).await?;
    let texture = load_texture(bytes).await?;
    let source_area =
        u64::from(texture.width().max(1) as u32) * u64::from(texture.height().max(1) as u32);
    let normalized = normalize_png_bytes(texture.save_to_png_bytes().as_ref())?;
    Ok(IconCandidate {
        bytes: normalized,
        source_area,
    })
}

pub async fn get_website_meta(url: Url) -> Result<WebsiteMeta> {
    let mut response = http_client()?.get_async(url.to_string()).await?;
    if !response.status().is_success() {
        bail!("{} returned HTTP {}", url, response.status());
    }
    let effective_url = response
        .effective_uri()
        .and_then(|uri| Url::parse(uri.to_string().as_str()).ok())
        .unwrap_or(url);
    let html =
        String::from_utf8_lossy(&read_bounded(&mut response, HTML_LIMIT).await?).into_owned();
    let document = Html::parse_document(&html);
    let title = document
        .select(title_selector())
        .next()
        .map(|element| element.text().collect::<String>())
        .map(|title| crate::model::sanitize_title(&title))
        .filter(|title| !title.is_empty());

    let mut urls = document
        .select(icon_selector())
        .filter_map(|element| element.attr("href"))
        .filter_map(|path| effective_url.join(path).ok())
        .collect::<HashSet<_>>();
    for path in ["/favicon.ico", "/favicon.png", "favicon.ico", "favicon.png"] {
        if let Ok(url) = effective_url.join(path) {
            urls.insert(url);
        }
    }
    let icon = stream::iter(urls)
        .map(fetch_icon)
        .buffer_unordered(MAX_ICON_REQUESTS)
        .filter_map(|result| async move { result.ok() })
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .max_by_key(|candidate| candidate.source_area)
        .map(|candidate| candidate.bytes);
    Ok(WebsiteMeta { icon, title })
}

pub fn valid_theme_color(value: &str) -> Option<String> {
    gdk::RGBA::parse(value.trim())
        .ok()
        .map(|color| color.to_string())
}

pub async fn icon_from_dialog(
    window: Option<&(impl IsA<gtk::Window> + Clone + 'static)>,
) -> Result<gio::File> {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(&gettext("Images")));
    filter.add_mime_type("image/*");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    gtk::FileDialog::builder()
        .accept_label(gettext("Select"))
        .modal(true)
        .title(gettext("App Icon"))
        .filters(&filters)
        .build()
        .open_future(window)
        .await
        .context("icon selection was cancelled")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    #[test]
    fn css_color_is_validated() {
        assert!(valid_theme_color("#3A7D78").is_some());
        assert!(
            valid_theme_color(include_str!("../tests/fixtures/invalid-theme-color.txt")).is_none()
        );
    }

    #[test]
    fn non_square_image_gets_transparent_padding() {
        let image = DynamicImage::new_rgba8(400, 100);
        let mut input = Cursor::new(Vec::new());
        image.write_to(&mut input, ImageFormat::Png).unwrap();
        let normalized = normalize_png_bytes(input.get_ref()).unwrap();
        let result = image::load_from_memory(&normalized).unwrap();
        assert_eq!(result.dimensions(), (ICON_SIZE, ICON_SIZE));
        assert_eq!(result.to_rgba8().get_pixel(0, 0).0[3], 0);
    }

    #[test]
    fn oversized_image_is_rejected_before_decode() {
        let result = futures::executor::block_on(normalize_icon(vec![0; IMAGE_LIMIT + 1]));
        assert!(result.is_err());
    }

    #[test]
    fn non_square_svg_is_normalized() {
        let fixture = include_bytes!("../tests/fixtures/non-square.svg").to_vec();
        let normalized = futures::executor::block_on(normalize_icon(fixture)).unwrap();
        let result = image::load_from_memory(&normalized).unwrap();
        assert_eq!(result.dimensions(), (ICON_SIZE, ICON_SIZE));
        assert_eq!(result.to_rgba8().get_pixel(0, 0).0[3], 0);
    }
}
