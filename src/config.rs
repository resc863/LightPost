use std::{
    env,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
};

use crate::time::parse_utc_offset;

/// The token the first version fell back to; refusing it keeps old setups from
/// silently running with a publicly known password.
const LEGACY_DEFAULT_TOKEN: &str = "change-me-in-production";
const RECOMMENDED_TOKEN_LENGTH: usize = 16;

/// Text that identifies the blog. Everything here can be changed without code edits.
#[derive(Debug, Clone)]
pub struct Site {
    pub title: String,
    /// Short line in the footer.
    pub tagline: String,
    /// Home page headline; line breaks are kept.
    pub headline: String,
    /// Home page introduction, `<meta name="description">` and the RSS channel description.
    pub description: String,
    pub lang: String,
}

impl Default for Site {
    fn default() -> Self {
        Self {
            title: "LightPost".to_owned(),
            tagline: "작은 기록이 쌓이는 곳.".to_owned(),
            headline: "작은 생각을 모아,\n오래 남을 기록으로.".to_owned(),
            description:
                "일상에서 발견한 것들, 배우며 쌓아가는 생각들.\n천천히 쓰고 오래 읽는 공간입니다."
                    .to_owned(),
            lang: "ko".to_owned(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub data_dir: PathBuf,
    pub admin_token: String,
    /// Public origin used for absolute links in RSS and the sitemap, without a trailing slash.
    pub base_url: String,
    pub site: Site,
    /// Offset used to show dates, in minutes east of UTC.
    pub utc_offset_minutes: i32,
    pub page_size: u32,
}

impl Config {
    /// Defaults for embedding and tests: localhost, Korean time, 10 posts per page.
    pub fn new(admin_token: impl Into<String>) -> Self {
        let bind_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 3000));
        Self {
            bind_addr,
            data_dir: PathBuf::from("data"),
            admin_token: admin_token.into(),
            base_url: format!("http://{bind_addr}"),
            site: Site::default(),
            utc_offset_minutes: 9 * 60,
            page_size: 10,
        }
    }

    /// The session cookie gets `Secure` when the public URL is https.
    pub fn secure_cookies(&self) -> bool {
        self.base_url.starts_with("https://")
    }

    /// Reads `LIGHTPOST_*` environment variables. See the README for the full list.
    pub fn from_env() -> Result<Self, String> {
        let admin_token = var("LIGHTPOST_ADMIN_TOKEN")
            .ok_or("LIGHTPOST_ADMIN_TOKEN must be set to the admin password")?;
        if admin_token == LEGACY_DEFAULT_TOKEN {
            return Err(format!(
                "LIGHTPOST_ADMIN_TOKEN is still the published default \"{LEGACY_DEFAULT_TOKEN}\"; choose a new one"
            ));
        }
        if admin_token.chars().count() < RECOMMENDED_TOKEN_LENGTH {
            eprintln!(
                "warning: LIGHTPOST_ADMIN_TOKEN is shorter than {RECOMMENDED_TOKEN_LENGTH} characters"
            );
        }

        let mut config = Self::new(admin_token);
        let host = match var("LIGHTPOST_HOST") {
            Some(value) => value
                .parse::<IpAddr>()
                .map_err(|_| format!("LIGHTPOST_HOST is not an IP address: {value}"))?,
            None => IpAddr::V4(Ipv4Addr::LOCALHOST),
        };
        let port = match var("LIGHTPOST_PORT") {
            Some(value) => value
                .parse::<u16>()
                .map_err(|_| format!("LIGHTPOST_PORT is not a port number: {value}"))?,
            None => 3000,
        };
        config.bind_addr = SocketAddr::new(host, port);

        if let Some(dir) = env::var_os("LIGHTPOST_DATA_DIR").filter(|value| !value.is_empty()) {
            config.data_dir = PathBuf::from(dir);
        }

        config.base_url = match var("LIGHTPOST_BASE_URL") {
            Some(value) => {
                if !value.starts_with("http://") && !value.starts_with("https://") {
                    return Err(format!(
                        "LIGHTPOST_BASE_URL must start with http:// or https://: {value}"
                    ));
                }
                value.trim_end_matches('/').to_owned()
            }
            None => {
                if host.is_unspecified() {
                    eprintln!(
                        "warning: set LIGHTPOST_BASE_URL so RSS and sitemap links point at the public address"
                    );
                }
                format!("http://{}", config.bind_addr)
            }
        };

        if let Some(value) = var("LIGHTPOST_UTC_OFFSET") {
            config.utc_offset_minutes = parse_utc_offset(&value)
                .ok_or_else(|| format!("LIGHTPOST_UTC_OFFSET must look like +09:00: {value}"))?;
        }
        if let Some(value) = var("LIGHTPOST_PAGE_SIZE") {
            config.page_size = value
                .parse::<u32>()
                .ok()
                .filter(|size| (1..=100).contains(size))
                .ok_or_else(|| format!("LIGHTPOST_PAGE_SIZE must be between 1 and 100: {value}"))?;
        }

        let site = &mut config.site;
        for (name, field) in [
            ("LIGHTPOST_SITE_TITLE", &mut site.title),
            ("LIGHTPOST_SITE_TAGLINE", &mut site.tagline),
            ("LIGHTPOST_SITE_HEADLINE", &mut site.headline),
            ("LIGHTPOST_SITE_DESCRIPTION", &mut site.description),
            ("LIGHTPOST_SITE_LANG", &mut site.lang),
        ] {
            if let Some(value) = var(name) {
                *field = value.replace("\\n", "\n");
            }
        }
        Ok(config)
    }
}

fn var(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}
