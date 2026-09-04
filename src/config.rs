// SPDX-License-Identifier: GPL-3.0-or-later

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GETTEXT_PACKAGE: &str = "bastle";
pub const APP_ID: &str = "io.github.cheviiot.bastle";
pub const DATA_DIR_NAME: &str = "bastle";

pub const LOCALEDIR: &str = match option_env!("BASTLE_LOCALEDIR") {
    Some(path) => path,
    None => "/usr/share/locale",
};

pub const PKGDATADIR: &str = match option_env!("BASTLE_PKGDATADIR") {
    Some(path) => path,
    None => "/usr/share/bastle",
};

pub fn managed_app_id(id: &crate::model::AppId) -> String {
    let component = if id
        .as_str()
        .starts_with(|character: char| character.is_ascii_digit())
    {
        format!("app{id}")
    } else {
        id.to_string()
    };
    format!("{APP_ID}.{component}")
}
